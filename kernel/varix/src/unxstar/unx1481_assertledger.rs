//! unx1481_assertledger —— UNX-F1481 域级断言总账建立（全域断言清点与总册）。
//! 锚点（CoRun Unxreal 主册 L3364）：UNX-F1481 · 域级断言总账建立（全域断言清点与总册） · 目标 320 行 · 已深化
//! 判据原文：UNX-F1481-J1 域级断言总账建成（全域 40 批断言逐批清点：断言号/判据号/类型
//!           三列入册），总账覆盖率 100%（批批有点），清点与各批收口报告对拍一致率 100%，
//!           漏点注入 10/10 必抓
//! 组件：断言总账三列登记（断言号/判据号/类型 · LedgerEntry 冻结三列）
//!      + 逐批清点机（B01–B40 批批有点 · 批位图 + 批内断言计数）
//!      + 收口报告对拍器（清点账 vs 各批收口侧账逐批对拍 · 双账一致率 100%）
//!      + 漏点注入检测核（注入缺批/缺条 10 场 · 检测器 10/10 必抓）
//! 判据落地：覆盖率 100%（批批有点）+ 对拍一致率 100% + 注入必抓 10/10
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u16/u64 整数算术，零墙钟（逻辑 tick 注入）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const AL_VERSION: &str = "unx1481_assertledger-v1";

/// 诊断段基址（0x4f40 段 · 本模块独占）。
pub const AL_SEG_BASE: u16 = 0x4f40;

/// 域批数（B01–B40）。
pub const AL_BATCH_COUNT: usize = 40;

/// 断言类型（三列之第三列 · 闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlKind {
    /// 判据型断言（J 系 · 判据原文直接断言）。
    Judge,
    /// 不变量型断言（I 系 · 结构不变量）。
    Invariant,
    /// 注入型断言（N 系 · 负向注入必抓）。
    Inject,
    /// 对拍型断言（P 系 · 双账对拍）。
    Pair,
}

impl AlKind {
    /// 类型缩写（入册登记用）。
    pub fn tag(self) -> &'static str {
        match self {
            AlKind::Judge => "J",
            AlKind::Invariant => "I",
            AlKind::Inject => "N",
            AlKind::Pair => "P",
        }
    }
}

/// 断言总账条目（三列冻结：断言号/判据号/类型）。
#[derive(Clone, Debug)]
pub struct AlEntry {
    /// 断言号（如 "UNX-F1481-A01"）。
    pub assert_id: String,
    /// 判据号（如 "UNX-F1481-J1"）。
    pub crit_id: String,
    /// 断言类型。
    pub kind: AlKind,
    /// 所属批次（0–39 → B01–B40）。
    pub batch: u16,
}

impl AlEntry {
    /// 行摘要（诊断输出）。
    pub fn line(&self) -> String {
        format!(
            "{}|{}|{}|B{:02}",
            self.assert_id, self.crit_id, self.kind.tag(), self.batch + 1
        )
    }
}

/// 单批清点结果（批位图 + 批内计数）。
#[derive(Clone, Copy, Debug)]
pub struct AlBatchTally {
    /// 批号（0–39）。
    pub batch: u16,
    /// 本批登记断言条数。
    pub entries: u32,
    /// 本批批内断言号唯一计数（重复登记不计二）。
    pub unique: u32,
}

impl AlBatchTally {
    /// 批批有点：本批至少登记 1 条。
    pub fn has_point(&self) -> bool {
        self.entries >= 1
    }
}

/// 逐批清点机：对总账全量条目做 40 批清点。
pub struct AlTallier {
    /// 各批清点结果（固定 40 槽）。
    pub tallies: [AlBatchTally; AL_BATCH_COUNT],
    /// 全账条目数。
    pub total: u32,
    /// 重复登记对数（同批同断言号出现 ≥2 次的对数）。
    pub dup_pairs: u32,
}

impl AlTallier {
    /// 空清点机。
    pub fn new() -> Self {
        AlTallier {
            tallies: core::array::from_fn(|i| AlBatchTally {
                batch: i as u16,
                entries: 0,
                unique: 0,
            }),
            total: 0,
            dup_pairs: 0,
        }
    }

    /// 全量清点：逐条入批计数 + 重复对统计。
    pub fn tally(&mut self, ledger: &[AlEntry]) {
        self.total = ledger.len() as u32;
        for t in self.tallies.iter_mut() {
            t.entries = 0;
            t.unique = 0;
        }
        self.dup_pairs = 0;
        // 批内断言号唯一化：按批聚合断言号首现即计 unique。
        for e in ledger {
            if (e.batch as usize) < AL_BATCH_COUNT {
                self.tallies[e.batch as usize].entries += 1;
            }
        }
        // 重复对：同批同断言号出现 k 次贡献 C(k,2) 中的 (k-1) 增量对。
        for b in 0..AL_BATCH_COUNT {
            let mut seen: Vec<&str> = Vec::new();
            let mut dup_b: u32 = 0;
            for e in ledger.iter().filter(|e| e.batch as usize == b) {
                if seen.iter().any(|s| *s == e.assert_id.as_str()) {
                    dup_b += 1;
                } else {
                    seen.push(e.assert_id.as_str());
                    self.tallies[b].unique += 1;
                }
            }
            self.dup_pairs += dup_b;
        }
    }

    /// 总账覆盖率：批批有点的批数 / 40（万分数）。
    pub fn coverage_permille(&self) -> u32 {
        let points = self.tallies.iter().filter(|t| t.has_point()).count() as u32;
        points * 1000 / AL_BATCH_COUNT as u32
    }

    /// 覆盖是否满格（40/40 批批有点）。
    pub fn coverage_full(&self) -> bool {
        self.tallies.iter().all(|t| t.has_point())
    }

    /// 零重复（登记卫生）。
    pub fn no_dup(&self) -> bool {
        self.dup_pairs == 0
    }
}

/// 收口侧账条目（各批收口报告声明的断言计数）。
#[derive(Clone, Copy, Debug)]
pub struct AlClosing {
    /// 批号。
    pub batch: u16,
    /// 收口报告声明条数。
    pub declared: u32,
}

/// 收口报告对拍器：清点账 vs 收口侧账逐批对拍。
pub struct AlPairer {
    /// 对拍一致的批数。
    pub agree: u32,
    /// 对拍不一致的批号表。
    pub diverge: Vec<u16>,
    /// 对拍批次总数。
    pub compared: u32,
}

impl AlPairer {
    /// 空对拍器。
    pub fn new() -> Self {
        AlPairer {
            agree: 0,
            diverge: Vec::new(),
            compared: 0,
        }
    }

    /// 逐批对拍：清点批内 unique 数 vs 收口声明数。
    pub fn pair(&mut self, tallier: &AlTallier, closing: &[AlClosing]) {
        self.agree = 0;
        self.diverge.clear();
        self.compared = 0;
        for c in closing {
            if (c.batch as usize) >= AL_BATCH_COUNT {
                continue;
            }
            self.compared += 1;
            let counted = tallier.tallies[c.batch as usize].unique;
            if counted == c.declared {
                self.agree += 1;
            } else {
                self.diverge.push(c.batch);
            }
        }
    }

    /// 对拍一致率（万分数；对拍 0 批视为不满格）。
    pub fn agree_rate(&self) -> u32 {
        if self.compared == 0 {
            return 0;
        }
        self.agree * 10000 / self.compared
    }

    /// 一致率满格（100%）。
    pub fn agree_full(&self) -> bool {
        self.compared >= AL_BATCH_COUNT as u32 && self.agree == self.compared
    }
}

/// 漏点注入场景（负向注入 · 检测器必须抓）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlInject {
    /// 整批漏登（该批 0 条 → 覆盖破格）。
    BatchOmit(u16),
    /// 批内部分漏登（收口声明 > 清点 → 对拍破裂）。
    EntryOmit(u16, u32),
    /// 收口虚报（收口声明 < 清点 → 对拍破裂反向）。
    ClosingShort(u16, u32),
    /// 重复登记（同一断言号双登 → 卫生破格）。
    DupEntry(u16),
}

impl AlInject {
    /// 场景名（诊断输出）。
    pub fn name(self) -> String {
        match self {
            AlInject::BatchOmit(b) => format!("batch-omit-B{:02}", b + 1),
            AlInject::EntryOmit(b, n) => format!("entry-omit-B{:02}-{}", b + 1, n),
            AlInject::ClosingShort(b, n) => format!("closing-short-B{:02}-{}", b + 1, n),
            AlInject::DupEntry(b) => format!("dup-entry-B{:02}", b + 1),
        }
    }
}

/// 漏点注入检测核：对每个注入场景构造污染账，检测器必须报警。
pub struct AlInjector {
    /// 注入场景数。
    pub cases: u32,
    /// 抓获数。
    pub caught: u32,
    /// 逐场景明细。
    pub log: Vec<String>,
}

impl AlInjector {
    /// 空注入核。
    pub fn new() -> Self {
        AlInjector {
            cases: 0,
            caught: 0,
            log: Vec::new(),
        }
    }

    /// 执行一轮注入：base 账为洁净底账，closing 为洁净收口账。
    /// 场景按序注入，检测器（覆盖率/对拍/卫生三闸）必须至少一闸报红。
    pub fn inject(&mut self, base: &[AlEntry], closing: &[AlClosing], scene: AlInject) {
        let mut ledger: Vec<AlEntry> = base.to_vec();
        let mut close: Vec<AlClosing> = closing.to_vec();
        let hit: bool;
        match scene {
            AlInject::BatchOmit(b) => {
                ledger.retain(|e| e.batch != b);
                let mut t = AlTallier::new();
                t.tally(&ledger);
                hit = !t.coverage_full();
            }
            AlInject::EntryOmit(b, n) => {
                let mut dropped = 0u32;
                ledger.retain(|e| {
                    if e.batch == b && dropped < n {
                        dropped += 1;
                        false
                    } else {
                        true
                    }
                });
                let mut t = AlTallier::new();
                t.tally(&ledger);
                let mut p = AlPairer::new();
                p.pair(&t, &close);
                hit = !p.agree_full() || !p.diverge.is_empty();
            }
            AlInject::ClosingShort(b, n) => {
                for c in close.iter_mut() {
                    if c.batch == b {
                        c.declared = c.declared.saturating_sub(n);
                    }
                }
                let mut t = AlTallier::new();
                t.tally(&ledger);
                let mut p = AlPairer::new();
                p.pair(&t, &close);
                hit = !p.agree_full() || !p.diverge.is_empty();
            }
            AlInject::DupEntry(b) => {
                if let Some(first) = ledger.iter().find(|e| e.batch == b).cloned() {
                    ledger.push(first);
                }
                let mut t = AlTallier::new();
                t.tally(&ledger);
                hit = !t.no_dup();
            }
        }
        self.cases += 1;
        if hit {
            self.caught += 1;
        }
        self.log
            .push(format!("{} -> {}", scene.name(), if hit { "CAUGHT" } else { "MISS" }));
    }

    /// 必抓满格（10/10 口径：全部场景必抓）。
    pub fn all_caught(&self) -> bool {
        self.cases >= 1 && self.caught == self.cases
    }
}

/// 域级断言总账门面（判据四件：三列入册/覆盖率/对拍/注入必抓）。
pub struct AlLedger {
    /// 总账条目。
    pub ledger: Vec<AlEntry>,
    /// 清点机。
    pub tallier: AlTallier,
    /// 对拍器。
    pub pairer: AlPairer,
    /// 注入核。
    pub injector: AlInjector,
}

impl AlLedger {
    /// 新总账。
    pub fn new() -> Self {
        AlLedger {
            ledger: Vec::new(),
            tallier: AlTallier::new(),
            pairer: AlPairer::new(),
            injector: AlInjector::new(),
        }
    }

    /// 登记一条断言（三列 + 批号）。
    pub fn enroll(&mut self, assert_id: &str, crit_id: &str, kind: AlKind, batch: u16) {
        self.ledger.push(AlEntry {
            assert_id: String::from(assert_id),
            crit_id: String::from(crit_id),
            kind,
            batch,
        });
    }

    /// 清点 + 对拍一气呵成。
    pub fn settle(&mut self, closing: &[AlClosing]) {
        self.tallier.tally(&self.ledger);
        self.pairer.pair(&self.tallier, closing);
    }

    /// 十场注入（判据 10/10 口径）：覆盖整批漏登×3/部分漏登×3/虚报×2/重复×2。
    pub fn inject_ten(&mut self, closing: &[AlClosing]) {
        let scenes = [
            AlInject::BatchOmit(3),
            AlInject::BatchOmit(17),
            AlInject::BatchOmit(31),
            AlInject::EntryOmit(5, 1),
            AlInject::EntryOmit(12, 2),
            AlInject::EntryOmit(26, 3),
            AlInject::ClosingShort(8, 1),
            AlInject::ClosingShort(22, 2),
            AlInject::DupEntry(9),
            AlInject::DupEntry(28),
        ];
        for s in scenes {
            self.injector.inject(&self.ledger, closing, s);
        }
    }

    /// 判据总门：覆盖率 100% + 对拍 100% + 注入 10/10 + 卫生零重复。
    pub fn judge(&self) -> (bool, String) {
        let cov = self.tallier.coverage_permille();
        let agr = self.pairer.agree_rate();
        let ok = self.tallier.coverage_full()
            && self.pairer.agree_full()
            && self.injector.all_caught()
            && self.tallier.no_dup();
        let msg = format!(
            "{}: cover={}/1000 agree={}/10000 inject={}/{} dup={} entries={} seg=0x{:04x}",
            AL_VERSION,
            cov,
            agr,
            self.injector.caught,
            self.injector.cases,
            self.tallier.dup_pairs,
            self.tallier.total,
            AL_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：40 批样例账全链路 + 十场注入。
pub fn run_t481_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1481-t481");
    let mut ledger = AlLedger::new();
    // 40 批逐批登记：每批 2 条（一 J 一 P），共 80 条。
    for b in 0..AL_BATCH_COUNT as u16 {
        ledger.enroll(
            &format!("UNX-F1481-A{:02}", b * 2 + 1),
            &format!("UNX-F1481-J1"),
            AlKind::Judge,
            b,
        );
        ledger.enroll(
            &format!("UNX-F1481-A{:02}", b * 2 + 2),
            &format!("UNX-B{:02}-P1", b + 1),
            AlKind::Pair,
            b,
        );
    }
    // 收口侧账：与清点 unique 一致（每批 2 条）。
    let closing: Vec<AlClosing> = (0..AL_BATCH_COUNT as u16)
        .map(|b| AlClosing {
            batch: b,
            declared: 2,
        })
        .collect();
    ledger.settle(&closing);
    ledger.inject_ten(&closing);
    let (ok, msg) = ledger.judge();
    set.add_dyn("UNX-F1481-J1", ok, &msg);
    // 子闸逐项落账。
    set.add("AL-coverage-full", ledger.tallier.coverage_full(), "");
    set.add("AL-agree-full", ledger.pairer.agree_full(), "");
    set.add("AL-inject-10/10", ledger.injector.all_caught(), "");
    set.add("AL-no-dup", ledger.tallier.no_dup(), "");
    set
}
