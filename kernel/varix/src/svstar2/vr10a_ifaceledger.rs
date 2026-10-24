//! VE-F3781 · R 域接口总账与主题映射终审（R10 收口组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3781`
//!
//! **判据（锚点原文）**：九族终版、双承诺、唯一性、留痕、家族对拍、判据。
//!
//! **本条是什么**：R 域九族接口清点的**终审总账**——册版本对账 +
//! 修正留痕 + 对外承诺表（双承诺）回执状态；主题映射终审
//! （十项落条唯一性断言）；五域映射家族对拍。对账非冻结即阻断。
//!
//! 诊断码段：独占 0xACxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xACxx）
// ─────────────────────────────────────────────────────────────

/// F3781 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ACCode(pub u16);

impl ACCode {
    /// 对账非冻结——阻断。
    pub const NOT_FROZEN: ACCode = ACCode(0xAC01);
    /// 映射缺/重——修正。
    pub const MAP_BAD: ACCode = ACCode(0xAC02);
    /// 回执未达——催办。
    pub const RECEIPT_MISS: ACCode = ACCode(0xAC03);
    /// 版本分歧。
    pub const VER_DIVERGE: ACCode = ACCode(0xAC04);
    /// 九族缺族。
    pub const FAMILY_MISS: ACCode = ACCode(0xAC05);
    /// 留痕缺失。
    pub const TRACE_MISS: ACCode = ACCode(0xAC06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xAC01 => "总账未冻结，后续工序阻断。",
            0xAC02 => "主题映射缺失或重复，需修正。",
            0xAC03 => "双承诺回执未达，需催办。",
            0xAC04 => "册版本分歧，需对账。",
            0xAC05 => "九族清点缺族。",
            0xAC06 => "修正留痕缺失。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 九族总账
// ─────────────────────────────────────────────────────────────

/// 接口族（九族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// 创作请求。
    Create,
    /// 工作流。
    Workflow,
    /// 编辑。
    Edit,
    /// 验证。
    Validate,
    /// 预览。
    Preview,
    /// 发布。
    Publish,
    /// 个性化。
    Persona,
    /// 市场。
    Market,
    /// 运行时。
    Runtime,
}

impl ApiFamily {
    /// 全集。
    pub const ALL: [ApiFamily; 9] = [
        ApiFamily::Create,
        ApiFamily::Workflow,
        ApiFamily::Edit,
        ApiFamily::Validate,
        ApiFamily::Preview,
        ApiFamily::Publish,
        ApiFamily::Persona,
        ApiFamily::Market,
        ApiFamily::Runtime,
    ];

    /// 序号。
    pub fn ordinal(self) -> usize {
        match self {
            ApiFamily::Create => 0,
            ApiFamily::Workflow => 1,
            ApiFamily::Edit => 2,
            ApiFamily::Validate => 3,
            ApiFamily::Preview => 4,
            ApiFamily::Publish => 5,
            ApiFamily::Persona => 6,
            ApiFamily::Market => 7,
            ApiFamily::Runtime => 8,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            ApiFamily::Create => "创作请求",
            ApiFamily::Workflow => "工作流",
            ApiFamily::Edit => "编辑",
            ApiFamily::Validate => "验证",
            ApiFamily::Preview => "预览",
            ApiFamily::Publish => "发布",
            ApiFamily::Persona => "个性化",
            ApiFamily::Market => "市场",
            ApiFamily::Runtime => "运行时",
        }
    }
}

/// 单族账目。
#[derive(Clone, Copy, Debug)]
pub struct FamilyLedger {
    /// 族。
    pub family: u8,
    /// 册版本。
    pub version: u16,
    /// 冻结。
    pub frozen: bool,
    /// 修正留痕条数。
    pub traces: u8,
    /// 对外承诺回执（双承诺：工具+产出）。
    pub receipt_tool: bool,
    pub receipt_output: bool,
}

impl FamilyLedger {
    /// 新建未冻。
    pub const fn new(family: u8) -> Self {
        FamilyLedger {
            family,
            version: 0,
            frozen: false,
            traces: 0,
            receipt_tool: false,
            receipt_output: false,
        }
    }

    /// 双承诺回执齐。
    pub fn receipts_ok(&self) -> bool {
        self.receipt_tool && self.receipt_output
    }

    /// 冻结前对账合法性（版本非零 + 有留痕）。
    pub fn ready_to_freeze(&self) -> bool {
        self.version > 0 && self.traces > 0
    }

    /// 执行冻结。
    pub fn freeze(&mut self) -> bool {
        if self.ready_to_freeze() {
            self.frozen = true;
            true
        } else {
            false
        }
    }
}

/// 总账（九族）。
#[derive(Clone, Debug)]
pub struct MasterLedger {
    families: Vec<FamilyLedger>,
}

impl MasterLedger {
    /// 新建。
    pub fn new() -> Self {
        MasterLedger {
            families: Vec::new(),
        }
    }

    /// 收一族。
    pub fn intake(&mut self, f: FamilyLedger) -> usize {
        self.families.push(f);
        self.families.len() - 1
    }

    /// 冻结数。
    pub fn frozen_count(&self) -> usize {
        self.families.iter().filter(|f| f.frozen).count()
    }

    /// 全冻。
    pub fn all_frozen(&self) -> bool {
        !self.families.is_empty() && self.frozen_count() == self.families.len()
    }

    /// 回执催办数（已冻但双承诺回执不齐）。
    pub fn receipt_pending(&self) -> usize {
        self.families
            .iter()
            .filter(|f| f.frozen && !f.receipts_ok())
            .count()
    }

    /// 版本分歧对数（同族重复且版本不同）。
    pub fn version_diverges(&self) -> usize {
        let mut n = 0;
        for i in 0..self.families.len() {
            for j in (i + 1)..self.families.len() {
                if self.families[i].family == self.families[j].family
                    && self.families[i].version != self.families[j].version
                {
                    n += 1;
                }
            }
        }
        n
    }

    /// 族覆盖数。
    pub fn family_coverage(&self) -> usize {
        let mut seen = [false; 9];
        for f in self.families.iter() {
            if (f.family as usize) < 9 {
                seen[f.family as usize] = true;
            }
        }
        seen.iter().filter(|s| **s).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.families.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.families.is_empty()
    }
}

impl Default for MasterLedger {
    fn default() -> Self {
        MasterLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 主题映射（十项落条唯一性）
// ─────────────────────────────────────────────────────────────

/// 主题映射表（十项 → 落条 id）。
#[derive(Clone, Debug)]
pub struct ThemeMap {
    /// 落条 id（0 表示未映射）。
    slots: [u16; 10],
}

impl ThemeMap {
    /// 新建。
    pub const fn new() -> Self {
        ThemeMap { slots: [0; 10] }
    }

    /// 落条（唯一性：目标 id 不得重复）。
    pub fn map(&mut self, theme: usize, target: u16) -> bool {
        if theme >= 10 || target == 0 {
            return false;
        }
        for s in self.slots.iter() {
            if *s == target {
                return false;
            }
        }
        self.slots[theme] = target;
        true
    }

    /// 已映射数。
    pub fn mapped(&self) -> usize {
        self.slots.iter().filter(|s| **s != 0).count()
    }

    /// 重复目标数。
    pub fn duplicates(&self) -> usize {
        let mut n = 0;
        for i in 0..10 {
            if self.slots[i] == 0 {
                continue;
            }
            for j in (i + 1)..10 {
                if self.slots[i] == self.slots[j] {
                    n += 1;
                }
            }
        }
        n
    }

    /// 缺口数。
    pub fn gaps(&self) -> usize {
        10 - self.mapped()
    }

    /// 唯一性成立。
    pub fn unique(&self) -> bool {
        self.duplicates() == 0
    }
}

// ─────────────────────────────────────────────────────────────
// 五域映射家族对拍
// ─────────────────────────────────────────────────────────────

/// 五域对拍器。
#[derive(Clone, Debug)]
pub struct DomainCompare {
    /// 五域各自落条指纹。
    prints: [u16; 5],
    /// 对拍通过标志。
    pub passed: bool,
}

impl DomainCompare {
    /// 新建。
    pub const fn new() -> Self {
        DomainCompare {
            prints: [0; 5],
            passed: false,
        }
    }

    /// 落一域指纹。
    pub fn stamp(&mut self, domain: usize, print: u16) -> bool {
        if domain < 5 {
            self.prints[domain] = print;
            true
        } else {
            false
        }
    }

    /// 对拍（五域指纹两两一致性——以首位为基准）。
    pub fn compare(&mut self) -> bool {
        let base = self.prints[0];
        for p in self.prints.iter() {
            if *p != base {
                self.passed = false;
                return false;
            }
        }
        self.passed = true;
        true
    }

    /// 分歧域数。
    pub fn diverge_count(&self) -> usize {
        let base = self.prints[0];
        self.prints.iter().filter(|p| **p != base).count()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3781 判据集。
pub fn run_vr10a_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3781");

    // 0xAC05 九族覆盖
    {
        let mut m = MasterLedger::new();
        set.add("0xAC05-初始空", m.is_empty(), "");
        for f in ApiFamily::ALL.iter() {
            let mut led = FamilyLedger::new(f.ordinal() as u8);
            led.version = 1;
            led.traces = 1;
            m.intake(led);
        }
        set.add("0xAC05-九族齐", m.family_coverage() == 9, "");
    }

    // 0xAC01 冻结门
    {
        let mut m = MasterLedger::new();
        let mut led = FamilyLedger::new(0);
        set.add("0xAC01-零版本不可冻", !led.freeze(), "");
        led.version = 2;
        set.add("0xAC01-无留痕不可冻", !led.freeze(), "");
        led.traces = 1;
        set.add("0xAC01-齐后可冻", led.freeze(), "");
        m.intake(led);
        set.add("0xAC01-单族已冻", m.frozen_count() == 1, "");
        set.add("0xAC01-未全冻", !m.all_frozen(), "");
    }

    // 0xAC03 回执催办
    {
        let mut m = MasterLedger::new();
        let mut led = FamilyLedger::new(1);
        led.version = 1;
        led.traces = 1;
        led.freeze();
        m.intake(led);
        set.add("0xAC03-单承诺不齐", m.receipt_pending() == 1, "");
        if let Some(f) = m.families.get_mut(0) {
            f.receipt_tool = true;
        }
        set.add("0xAC03-半回执仍催", m.receipt_pending() == 1, "");
        if let Some(f) = m.families.get_mut(0) {
            f.receipt_output = true;
        }
        set.add("0xAC03-双回执清", m.receipt_pending() == 0, "");
    }

    // 0xAC04 版本分歧
    {
        let mut m = MasterLedger::new();
        let mut a = FamilyLedger::new(2);
        a.version = 1;
        let mut b = FamilyLedger::new(2);
        b.version = 2;
        m.intake(a);
        m.intake(b);
        set.add("0xAC04-分歧检出", m.version_diverges() == 1, "");
    }

    // 0xAC02 映射唯一性
    {
        let mut t = ThemeMap::new();
        set.add("0xAC02-初始零映", t.mapped() == 0, "");
        set.add("0xAC02-零目标拒", !t.map(0, 0), "");
        set.add("0xAC02-首落", t.map(0, 100), "");
        set.add("0xAC02-重目标拒", !t.map(1, 100), "");
        set.add("0xAC02-唯一成立", t.unique() && t.duplicates() == 0, "");
        for i in 1..10 {
            t.map(i, 100 + i as u16);
        }
        set.add("0xAC02-十项落满", t.mapped() == 10 && t.gaps() == 0, "");
    }

    // 0xAC04 五域对拍
    {
        let mut d = DomainCompare::new();
        for i in 0..5 {
            d.stamp(i, 77);
        }
        set.add("0xAC04-同指纹过", d.compare(), "");
        set.add("0xAC04-零分歧", d.diverge_count() == 0, "");
        d.stamp(3, 88);
        set.add("0xAC04-分歧不过", !d.compare(), "");
        set.add("0xAC04-一分歧", d.diverge_count() == 1, "");
    }

    // 族序自洽
    {
        let mut ok = true;
        for (i, f) in ApiFamily::ALL.iter().enumerate() {
            if f.ordinal() != i {
                ok = false;
            }
        }
        set.add("0xAC05-族序自洽", ok, "");
    }

    set
}
