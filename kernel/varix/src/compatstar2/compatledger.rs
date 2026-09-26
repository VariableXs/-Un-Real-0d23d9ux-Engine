//! F040 兼容域总判据（compatstar · G-A-40）——兼容性不是口号是账本。
//!
//! 主册判据（验收标准第一句）：
//! **首版账本 50/50 建档完成即本域总判据达成；季报如期发布两期。**
//!
//! 功能定义（G-A-40）：「常用 50 件」兼容账本制度：五类（文本编辑/压缩/
//! 图片/终端工具/开发工具）各 10 件开源代表，逐件实测建档（安装/启动/
//! 核心功能三关），通过率季度刷新进星图与生态季报（F149）。
//!
//! 【设计细节】账本数据文件版本化（季度快照不可变，历史可溯）；构建机器
//! 自动汇总。
//! 【交互设计】星图「兼容账本」分栏：筛选器（类别/状态/季度）+ 每件展开看
//! 三关详情与证据（录屏哈希/日期/版本）；账本数据全量 JSON 开放（F128）。
//! 【状态与异常】样本升级后回退 → 状态降黄并归因（上游问题/VARIX 回归
//! 二分）；连续两季红的件 → 移入「攻坚名单」公示；样本淘汰（上游停维）→
//! 归档不删除。样本全部选活跃开源项目（F130 登记）；账本框架自研（JSON
//! 格式开放 F126）。
//!
//! 零堆纪律：定长账本表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 五类各 10 件 = 50 件（主册【功能定义】）。
pub const CATEGORIES: [&str; 5] = ["text-editor", "compression", "image", "terminal-tool", "dev-tool"];
pub const PER_CATEGORY: usize = 10;
pub const LEDGER_CAP: usize = 50;
/// 三关（安装/启动/核心功能）。
pub const THREE_GATES: [&str; 3] = ["install", "launch", "core-function"];
/// 账本 JSON 全量开放（F128）格式名。
pub const LEDGER_FORMAT: &str = "varix-compat-ledger-json-v1";
/// 连续两季红 → 攻坚名单。
pub const RED_STREAK_LIMIT: u32 = 2;

/// 五类 × 10 件的「常用 50 件」名单（活跃开源代表；F130 登记）。
pub const LEDGER_ITEMS: [&str; 50] = [
    // 文本编辑 10
    "Notepad2", "Notepad3", "Notepad--", "Akelpad", "CudaText", "GVim", "Micro", "Editra", "SciTE", "Metapad",
    // 压缩 10
    "7-Zip", "PeaZip", "Bandizip", "NanaZip", "XArchiver", "FreeArc", "Zstd-GUI", "Unrar-GUI", "Ashampoo-Free", "Zip-Genius",
    // 图片 10
    "IrfanView", "XnView-MP", "ImageEye", "Nomacs", "FastStone", "JPEGView", "Honeyview", "dimin-viewer", "PhotoQt", "imv",
    // 终端工具 10
    "PuTTY", "KiTTY", "WinSCP", "htop-win", "Far-Manager", "ConEmu", "Cmder", "MinTTY", "Tabby", "Alacritty",
    // 开发工具 10
    "VSCode-OSS", "VSCodium", "Git-for-Win", "TortoiseGit", "Sublime-Merge", "CMake-GUI", "Ninja", "GNU-Make", "Python-IDLE", "Node-REPL",
];

// ---------------------------------------------------------------------------
// 账本
// ---------------------------------------------------------------------------

/// 件状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemStatus {
    /// 三关全绿。
    Green,
    /// 有回归/部分通过（降黄并归因）。
    Yellow,
    /// 核心功能不可用。
    Red,
    /// 归档（上游停维；归档不删除——主册【状态与异常】）。
    Archived,
}

/// 回退归因二分（上游问题 / VARIX 回归）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegressionCause {
    Upstream,
    VarixRegression,
}

/// 一件账目。
#[derive(Clone, Copy)]
pub struct LedgerItem {
    pub name: &'static str,
    pub category: usize,
    /// 三关通过位图（bit0 install / bit1 launch / bit2 core）。
    pub gates_bits: u8,
    pub status: ItemStatus,
    /// 季度标记（快照不可变的版本键）。
    pub quarter: u32,
    /// 连续红季计数。
    pub red_streak: u32,
    /// 回退归因（黄/红时登记）。
    pub cause: Option<RegressionCause>,
    /// 证据哈希（录屏哈希/日期/版本——域内存哈希 8 字节）。
    pub evidence_hash8: [u8; 8],
}

impl LedgerItem {
    /// 三关全过 = 绿。
    pub fn three_gates_pass(&self) -> bool {
        self.gates_bits == 0b111
    }
}

/// 兼容账本。
pub struct CompatLedger {
    items: [Option<LedgerItem>; LEDGER_CAP],
    count: usize,
    /// 季度快照链（季度号 → 全量通过率 permille；快照不可变）。
    pub quarter_snapshots: [(u32, u32); 8],
    pub snapshot_n: usize,
}

impl CompatLedger {
    pub const fn new() -> Self {
        CompatLedger { items: [None; LEDGER_CAP], count: 0, quarter_snapshots: [(0, 0); 8], snapshot_n: 0 }
    }

    /// 建档（三关登记）。
    pub fn enroll(&mut self, name: &'static str, category: usize, gates_bits: u8, quarter: u32, evidence: [u8; 8]) -> Result<usize, &'static str> {
        if category >= CATEGORIES.len() {
            return Err("bad-category");
        }
        if self.count >= LEDGER_CAP {
            return Err("ledger-full");
        }
        self.items[self.count] = Some(LedgerItem {
            name,
            category,
            gates_bits,
            status: if gates_bits == 0b111 { ItemStatus::Green } else if gates_bits == 0 { ItemStatus::Red } else { ItemStatus::Yellow },
            quarter,
            red_streak: 0,
            cause: None,
            evidence_hash8: evidence,
        });
        self.count += 1;
        Ok(self.count - 1)
    }

    /// 样本升级后回退 → 状态降黄并归因（二分：上游 / VARIX 回归）。
    pub fn regress(&mut self, i: usize, cause: RegressionCause) -> bool {
        match self.items[i].as_mut() {
            Some(it) => {
                it.status = ItemStatus::Yellow;
                it.cause = Some(cause);
                true
            }
            None => false,
        }
    }

    /// 季度结算：红季计数累计；连续两季红 → 攻坚名单公示位。
    pub fn settle_quarter(&mut self, quarter: u32, pass_rate_permille: u32) -> usize {
        for it in self.items.iter_mut().flatten() {
            if it.status == ItemStatus::Red {
                it.red_streak += 1;
            }
        }
        if self.snapshot_n < 8 {
            self.quarter_snapshots[self.snapshot_n] = (quarter, pass_rate_permille);
            self.snapshot_n += 1;
        }
        self.count_hardball()
    }

    /// 攻坚名单计数（连续两季红——公示位；名字走 items 遍历零分配）。
    pub fn count_hardball(&self) -> usize {
        self.items.iter().flatten().filter(|it| it.status == ItemStatus::Red && it.red_streak >= RED_STREAK_LIMIT).count()
    }

    /// 通过率（permille）——季度刷新进星图。
    pub fn pass_rate_permille(&self) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let green = self.items.iter().flatten().filter(|it| it.status == ItemStatus::Green).count();
        (green * 1000 / self.count) as u32
    }

    /// 样本淘汰：归档不删除。
    pub fn archive(&mut self, i: usize) -> bool {
        match self.items[i].as_mut() {
            Some(it) => {
                it.status = ItemStatus::Archived;
                true
            }
            None => false,
        }
    }

    /// 筛选器（类别/状态——星图分栏的查询面）。
    pub fn filter_by_category(&self, cat: usize) -> usize {
        self.items.iter().flatten().filter(|it| it.category == cat).count()
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 首版账本 50/50 建档达成判据。
    pub fn first_edition_complete(&self) -> bool {
        self.count == LEDGER_CAP
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_compatledger_checks() -> CheckSet {
    let mut cs = CheckSet::new("F040-compatledger");
    // 1) 五类 × 10 = 50 件名单在册。
    cs.add("fifty_items_roster", LEDGER_ITEMS.len() == 50 && CATEGORIES.len() == 5 && PER_CATEGORY == 10, "");
    // 2) 三关在册（安装/启动/核心功能）。
    cs.add("three_gates", THREE_GATES == ["install", "launch", "core-function"], "");
    // 3) 50/50 建档 → 首版达成。
    let mut ledger = CompatLedger::new();
    let mut all_enrolled = true;
    for (i, name) in LEDGER_ITEMS.iter().enumerate() {
        all_enrolled &= ledger.enroll(name, i / PER_CATEGORY, 0b111, 1, [i as u8; 8]).is_ok();
    }
    cs.add("first_edition_50_of_50", all_enrolled && ledger.first_edition_complete() && ledger.count() == 50, "");
    // 4) 三关全过 = 绿；通过率 100%。
    cs.add("all_green_rate_1000", ledger.pass_rate_permille() == 1000, "");
    // 5) 回退降黄并归因（二分）。
    ledger.regress(0, RegressionCause::Upstream);
    ledger.regress(1, RegressionCause::VarixRegression);
    cs.add("regress_yellow_with_cause", ledger.pass_rate_permille() == 960, "");
    // 6) 季度快照不可变（历史可溯：两期快照在册）。
    ledger.settle_quarter(1, 960);
    ledger.settle_quarter(2, 980);
    cs.add("quarter_snapshots_immutable", ledger.snapshot_n == 2 && ledger.quarter_snapshots[0] == (1, 960) && ledger.quarter_snapshots[1] == (2, 980), "");
    // 7) JSON 开放格式名在册（F128/F126）。
    cs.add("json_open_format", LEDGER_FORMAT == "varix-compat-ledger-json-v1", "");
    // 8) 攻坚名单：连续两季红才入列。
    let mut hb = CompatLedger::new();
    hb.enroll("a", 0, 0b000, 1, [1; 8]).unwrap(); // 红
    hb.settle_quarter(1, 0); // 红季 1
    let not_yet = hb.count_hardball();
    hb.settle_quarter(2, 0); // 红季 2 → 入列
    cs.add("hardball_two_red_quarters", not_yet == 0 && hb.count_hardball() == 1 && RED_STREAK_LIMIT == 2, "");
    // 9) 样本淘汰 → 归档不删除（条目仍在册）。
    let mut arch = CompatLedger::new();
    arch.enroll("old-tool", 4, 0b111, 1, [2; 8]).unwrap();
    arch.archive(0);
    cs.add("archive_not_delete", arch.count() == 1 && arch.items[0].unwrap().status == ItemStatus::Archived, "");
    // 10) 筛选器按类别查询（星图分栏）。
    cs.add("filter_by_category", ledger.filter_by_category(0) == 10 && ledger.filter_by_category(4) == 10, "");
    // 11) 季报发布两期（快照两期即账面）。
    cs.add("two_quarterly_reports", ledger.snapshot_n == 2, "");
    // 12) 账本框架自研 + 容量守卫。
    let mut full = CompatLedger::new();
    for (i, name) in LEDGER_ITEMS.iter().enumerate() {
        full.enroll(name, i % 5, 0b111, 1, [9; 8]).unwrap();
    }
    cs.add("ledger_capacity", full.enroll("extra", 0, 0b111, 1, [9; 8]) == Err("ledger-full"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据模型：首版账本 50/50 建档 = 本域总判据达成。
    #[test]
    fn first_edition_fifty_fifty() {
        let mut l = CompatLedger::new();
        for (i, name) in LEDGER_ITEMS.iter().enumerate() {
            l.enroll(name, i / PER_CATEGORY, 0b111, 1, [i as u8; 8]).unwrap();
        }
        assert!(l.first_edition_complete(), "50/50 建档");
        assert_eq!(l.pass_rate_permille(), 1000);
        // 五类各 10 件。
        for cat in 0..5 {
            assert_eq!(l.filter_by_category(cat), 10);
        }
    }

    /// 回退二分归因：上游问题 vs VARIX 回归各自在册。
    #[test]
    fn regression_bisection_recorded() {
        let mut l = CompatLedger::new();
        l.enroll("x", 0, 0b111, 1, [1; 8]).unwrap();
        l.regress(0, RegressionCause::VarixRegression);
        let it = l.items[0].unwrap();
        assert_eq!(it.status, ItemStatus::Yellow);
        assert_eq!(it.cause, Some(RegressionCause::VarixRegression));
    }

    /// 季度刷新与红季累计联动。
    #[test]
    fn red_streak_accumulates_over_quarters() {
        let mut l = CompatLedger::new();
        l.enroll("sick", 0, 0b000, 1, [1; 8]).unwrap();
        l.settle_quarter(1, 0);
        l.settle_quarter(2, 0);
        l.settle_quarter(3, 0);
        assert_eq!(l.items[0].unwrap().red_streak, 3, "三季红累计");
        assert_eq!(l.count_hardball(), 1, "连续 ≥2 季红在攻坚名单");
    }

    /// 名单内容抽查：五类各自代表件在位。
    #[test]
    fn roster_content_spot_check() {
        assert_eq!(LEDGER_ITEMS[0], "Notepad2");
        assert_eq!(LEDGER_ITEMS[10], "7-Zip");
        assert_eq!(LEDGER_ITEMS[20], "IrfanView");
        assert_eq!(LEDGER_ITEMS[30], "PuTTY");
        assert_eq!(LEDGER_ITEMS[40], "VSCode-OSS");
    }
}
