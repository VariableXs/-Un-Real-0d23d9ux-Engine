//! VE-F3792 · R 域清账确认（R10 收口组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3792`
//!
//! **判据（锚点原文）**：十套不虚记、六账双签、全量扫描零豁免、零偏差、
//! 批台账、判据。
//!
//! **本条是什么**：清账终审——十套账目不虚记断言；六账双签核销；全量扫描
//! 零豁免；两零偏差对账；批台账。缺账即补登，虚记即撤回。
//!
//! 诊断码段：独占 0xB7xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xB7xx）
// ─────────────────────────────────────────────────────────────

/// F3792 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B7Code(pub u16);

impl B7Code {
    /// 缺账——补登。
    pub const BOOK_MISS: B7Code = B7Code(0xB701);
    /// 虚记——撤回。
    pub const FALSE_ENTRY: B7Code = B7Code(0xB702);
    /// 双签缺。
    pub const SIGN_MISS: B7Code = B7Code(0xB703);
    /// 偏差非零——对账。
    pub const DEVIATION: B7Code = B7Code(0xB704);
    /// 豁免检出——P0。
    pub const EXEMPT_FOUND: B7Code = B7Code(0xB705);
    /// 批台账缺。
    pub const BATCH_MISS: B7Code = B7Code(0xB706);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xB701 => "清账缺账，需补登。",
            0xB702 => "虚记检出，须撤回。",
            0xB703 => "双签缺失，清账不生效。",
            0xB704 => "清账偏差非零，需对账。",
            0xB705 => "豁免检出，P0 红线。",
            0xB706 => "批台账缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 十套账目
// ─────────────────────────────────────────────────────────────

/// 账目套（十套）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookSet {
    /// 接口账。
    Api,
    /// 文档账。
    Doc,
    /// 判据账。
    Criteria,
    /// 安全账。
    Sec,
    /// 无障碍账。
    A11y,
    /// 性能账。
    Perf,
    /// 观测账。
    Obs,
    /// 变更账。
    Chg,
    /// 预留账。
    Rsv,
    /// 案例账。
    Case,
}

impl BookSet {
    /// 全集。
    pub const ALL: [BookSet; 10] = [
        BookSet::Api,
        BookSet::Doc,
        BookSet::Criteria,
        BookSet::Sec,
        BookSet::A11y,
        BookSet::Perf,
        BookSet::Obs,
        BookSet::Chg,
        BookSet::Rsv,
        BookSet::Case,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            BookSet::Api => 0,
            BookSet::Doc => 1,
            BookSet::Criteria => 2,
            BookSet::Sec => 3,
            BookSet::A11y => 4,
            BookSet::Perf => 5,
            BookSet::Obs => 6,
            BookSet::Chg => 7,
            BookSet::Rsv => 8,
            BookSet::Case => 9,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            BookSet::Api => "接口账",
            BookSet::Doc => "文档账",
            BookSet::Criteria => "判据账",
            BookSet::Sec => "安全账",
            BookSet::A11y => "无障碍账",
            BookSet::Perf => "性能账",
            BookSet::Obs => "观测账",
            BookSet::Chg => "变更账",
            BookSet::Rsv => "预留账",
            BookSet::Case => "案例账",
        }
    }
}

/// 单套账状态。
#[derive(Clone, Copy, Debug)]
pub struct BookStatus {
    /// 在册。
    pub present: bool,
    /// 条目数。
    pub entries: u32,
    /// 虚记数。
    pub falsified: u32,
    /// 双签（制单+复核）。
    pub sign_make: bool,
    pub sign_check: bool,
}

impl BookStatus {
    /// 新建缺。
    pub const fn empty() -> Self {
        BookStatus {
            present: false,
            entries: 0,
            falsified: 0,
            sign_make: false,
            sign_check: false,
        }
    }

    /// 双签齐。
    pub fn signed(&self) -> bool {
        self.sign_make && self.sign_check
    }

    /// 不虚记。
    pub fn honest(&self) -> bool {
        self.falsified == 0
    }
}

/// 清账簿（十套）。
#[derive(Clone, Debug)]
pub struct ClearBook {
    sets: [BookStatus; 10],
    /// 批台账在册。
    pub batch_ledger: bool,
}

impl ClearBook {
    /// 新建。
    pub fn new() -> Self {
        ClearBook {
            sets: [BookStatus::empty(); 10],
            batch_ledger: false,
        }
    }

    /// 登记一套。
    pub fn enroll(&mut self, b: BookSet, entries: u32) -> bool {
        let i = b.ordinal();
        self.sets[i].present = true;
        self.sets[i].entries = entries;
        true
    }

    /// 缺账数。
    pub fn missing(&self) -> usize {
        self.sets.iter().filter(|s| !s.present).count()
    }

    /// 虚记总数。
    pub fn total_falsified(&self) -> u32 {
        self.sets.iter().map(|s| s.falsified).sum()
    }

    /// 双签齐套数。
    pub fn signed_count(&self) -> usize {
        self.sets.iter().filter(|s| s.signed()).count()
    }

    /// 全双签。
    pub fn all_signed(&self) -> bool {
        self.signed_count() == 10
    }

    /// 某套可变。
    pub fn get_mut(&mut self, b: BookSet) -> &mut BookStatus {
        &mut self.sets[b.ordinal()]
    }
}

impl Default for ClearBook {
    fn default() -> Self {
        ClearBook::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 全量扫描零豁免 + 两零偏差
// ─────────────────────────────────────────────────────────────

/// 扫描结果。
#[derive(Clone, Copy, Debug)]
pub struct ScanResult {
    /// 扫描条数。
    pub scanned: u32,
    /// 豁免数（须 0）。
    pub exempt: u32,
    /// 实账偏差（须 0）。
    pub real_dev: i32,
    /// 记录偏差（须 0）。
    pub rec_dev: i32,
}

impl ScanResult {
    /// 新建。
    pub const fn new() -> Self {
        ScanResult {
            scanned: 0,
            exempt: 0,
            real_dev: 0,
            rec_dev: 0,
        }
    }

    /// 零豁免。
    pub fn zero_exempt(&self) -> bool {
        self.exempt == 0
    }

    /// 两零偏差。
    pub fn zero_dev(&self) -> bool {
        self.real_dev == 0 && self.rec_dev == 0
    }

    /// 全过。
    pub fn pass(&self) -> bool {
        self.scanned > 0 && self.zero_exempt() && self.zero_dev()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3792 判据集。
pub fn run_vr10l_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3792");

    // 0xB701 十套在册
    {
        let mut c = ClearBook::new();
        set.add("0xB701-初始缺十", c.missing() == 10, "");
        for b in BookSet::ALL.iter() {
            c.enroll(*b, 10);
        }
        set.add("0xB701-十套齐", c.missing() == 0, "");
    }

    // 0xB702 不虚记
    {
        let mut c = ClearBook::new();
        for b in BookSet::ALL.iter() {
            c.enroll(*b, 5);
        }
        set.add("0xB702-初始诚实", c.total_falsified() == 0, "");
        c.get_mut(BookSet::Doc).falsified = 2;
        set.add("0xB702-虚记检出", c.total_falsified() == 2, "");
        c.get_mut(BookSet::Doc).falsified = 0;
        set.add("0xB702-撤回后清", c.total_falsified() == 0, "");
    }

    // 0xB703 双签
    {
        let mut c = ClearBook::new();
        for b in BookSet::ALL.iter() {
            c.enroll(*b, 1);
        }
        set.add("0xB703-初始未签", c.signed_count() == 0, "");
        for b in BookSet::ALL.iter() {
            let s = c.get_mut(*b);
            s.sign_make = true;
            s.sign_check = true;
        }
        set.add("0xB703-十套双签", c.all_signed() && c.signed_count() == 10, "");
    }

    // 0xB705 零豁免
    {
        let s = ScanResult {
            scanned: 100,
            exempt: 0,
            real_dev: 0,
            rec_dev: 0,
        };
        set.add("0xB705-零豁免过", s.zero_exempt() && s.pass(), "");
        let s2 = ScanResult {
            scanned: 100,
            exempt: 1,
            real_dev: 0,
            rec_dev: 0,
        };
        set.add("0xB705-豁免不过", !s2.zero_exempt() && !s2.pass(), "");
    }

    // 0xB704 两零偏差
    {
        let s = ScanResult {
            scanned: 50,
            exempt: 0,
            real_dev: 3,
            rec_dev: -2,
        };
        set.add("0xB704-偏差非零", !s.zero_dev(), "");
        let s2 = ScanResult {
            scanned: 50,
            exempt: 0,
            real_dev: 0,
            rec_dev: 0,
        };
        set.add("0xB704-两零过", s2.zero_dev() && s2.pass(), "");
        let s3 = ScanResult {
            scanned: 0,
            exempt: 0,
            real_dev: 0,
            rec_dev: 0,
        };
        set.add("0xB704-零扫描不过", !s3.pass(), "");
    }

    // 0xB706 批台账
    {
        let mut c = ClearBook::new();
        set.add("0xB706-初始无台账", !c.batch_ledger, "");
        c.batch_ledger = true;
        set.add("0xB706-台账在", c.batch_ledger, "");
    }

    // 套序自洽
    {
        let mut ok = true;
        for (i, b) in BookSet::ALL.iter().enumerate() {
            if b.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xB701-套序自洽", ok, "");
    }

    set
}
