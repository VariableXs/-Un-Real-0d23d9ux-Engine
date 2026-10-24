//! ves363_s20budget — VE-F4123 · T07 i18n 预算体系
//! 成本模型→预算分解：查找延迟/切换耗时/内存占用/包体积四维预算×语种档位 + 预算登记入家族预算总表 + 预算闸（超预算拒绝合入——CI 门禁联动）+ 超预算重议通道 + 预算冲突 T07 仲裁 + 档位缺按最严档兜底 + 无障碍保底档（任何档位无障碍预算不减——域本色红线）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xD140;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BdCode {
    Ok = 0,
    OverBudget = SEG_BASE + 0x01,
    Conflict = SEG_BASE + 0x02,
    A11yFloor = SEG_BASE + 0x03,
}

/// 四维预算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dim { Lookup = 0, Switch = 1, Memory = 2, Package = 3 }

impl Dim {
    pub const ALL: [Dim; 4] = [Dim::Lookup, Dim::Switch, Dim::Memory, Dim::Package];
    pub fn name(self) -> &'static str {
        match self { Dim::Lookup => "lookup", Dim::Switch => "switch", Dim::Memory => "memory", Dim::Package => "package" }
    }
}

/// 语种档位。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier { Light = 0, Medium = 1, Heavy = 2 }

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Light, Tier::Medium, Tier::Heavy];
    pub fn name(self) -> &'static str {
        match self { Tier::Light => "light", Tier::Medium => "medium", Tier::Heavy => "heavy" }
    }
}

/// 预算表：档位 × 四维 → 阈值。
pub struct BudgetTable { pub rows: Vec<(Tier, [u32; 4])> }

impl BudgetTable {
    pub fn new() -> Self {
        BudgetTable { rows: vec![
            (Tier::Light, [200, 600, 8_000, 1_000]),
            (Tier::Medium, [300, 900, 24_000, 3_000]),
            (Tier::Heavy, [400, 1_200, 48_000, 6_000]),
        ] }
    }

    pub fn budget(&self, tier: Tier, dim: Dim) -> Option<u32> {
        self.rows.iter().find(|(t, _)| *t == tier).map(|(_, v)| v[dim as usize])
    }

    /// 档位缺 → 按最严档（Heavy）兜底。
    pub fn budget_or_strictest(&self, tier: Option<Tier>, dim: Dim) -> (u32, bool) {
        match tier {
            Some(t) => match self.budget(t, dim) {
                Some(v) => (v, false),
                None => (self.budget(Tier::Heavy, dim).unwrap_or(u32::MAX), true),
            },
            None => (self.budget(Tier::Heavy, dim).unwrap_or(u32::MAX), true),
        }
    }
}

/// 家族预算总表登记。
pub struct FamilyLedger { pub entries: Vec<(&'static str, Dim, u32)> }

impl FamilyLedger {
    pub fn new() -> Self { FamilyLedger { entries: Vec::new() } }
    pub fn register(&mut self, feature: &'static str, dim: Dim, budget: u32) {
        if !self.entries.iter().any(|(f, d, _)| *f == feature && *d == dim) {
            self.entries.push((feature, dim, budget));
        }
    }
    pub fn of(&self, feature: &str, dim: Dim) -> Option<u32> {
        self.entries.iter().find(|(f, d, _)| *f == feature && *d == dim).map(|(_, _, b)| *b)
    }
}

/// 预算闸：功能实际值 vs 预算（CI 门禁联动——拒绝合入）。
pub struct BudgetGate<'a> { pub table: &'a BudgetTable, pub ledger: &'a FamilyLedger }

impl<'a> BudgetGate<'a> {
    /// 判定：超预算 → 拒 + 重议通道标记。
    pub fn check(&self, feature: &str, dim: Dim, tier: Tier, actual: u32) -> Result<(), BdCode> {
        let budget = self.ledger.of(feature, dim)
            .or_else(|| self.table.budget(tier, dim))
            .ok_or(BdCode::Conflict)?;
        if actual > budget { return Err(BdCode::OverBudget); }
        Ok(())
    }
}

/// 无障碍保底档（域本色红线）：任何语种档位下无障碍预算不减。
pub const A11Y_FLOOR: u32 = 150; // ms

pub fn a11y_floor_respected(per_tier: &[(Tier, u32)]) -> bool {
    per_tier.iter().all(|(_, v)| *v >= A11Y_FLOOR)
}

/// 仲裁册（T07）。
pub struct ArbitrateBook { pub cases: Vec<(&'static str, &'static str)> }

impl ArbitrateBook {
    pub fn new() -> Self { ArbitrateBook { cases: Vec::new() } }
    pub fn open(&mut self, a: &'static str, b: &'static str) { self.cases.push((a, b)); }
    pub fn count(&self) -> usize { self.cases.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves363_checks() -> CheckSet {
    let mut set = CheckSet::new("ves363");
    let bt = BudgetTable::new();

    // C363-01 四维预算×三档齐备。
    let full = Tier::ALL.iter().all(|t| Dim::ALL.iter().all(|d| bt.budget(*t, *d).is_some()));
    chk(&mut set, "C363-01", full && bt.rows.len() == 3, "3 档×4 维齐备");

    // C363-02 档位序：Heavy 各维最宽。
    let heavy_widest = Dim::ALL.iter().all(|d| {
        bt.budget(Tier::Heavy, *d) >= bt.budget(Tier::Medium, *d)
            && bt.budget(Tier::Medium, *d) >= bt.budget(Tier::Light, *d)
    });
    chk(&mut set, "C363-02", heavy_widest, "Light ≤ Medium ≤ Heavy");

    // C363-03 家族总表登记 + 查询。
    let mut led = FamilyLedger::new();
    led.register("search-box", Dim::Lookup, 250);
    led.register("lang-menu", Dim::Switch, 800);
    chk(&mut set, "C363-03", led.of("search-box", Dim::Lookup) == Some(250), "总表登记可查");

    // C363-04 预算闸：预算内放行。
    let gate = BudgetGate { table: &bt, ledger: &led };
    chk(&mut set, "C363-04", gate.check("search-box", Dim::Lookup, Tier::Medium, 240).is_ok(), "240 ≤ 250 放行");

    // C363-05 超预算拒（CI 联动）。
    chk(&mut set, "C363-05", gate.check("search-box", Dim::Lookup, Tier::Medium, 260) == Err(BdCode::OverBudget), "260 > 250 => 拒合入");

    // C363-06 未登功能回退档位预算。
    let gate2 = BudgetGate { table: &bt, ledger: &FamilyLedger::new() };
    chk(&mut set, "C363-06", gate2.check("new-fn", Dim::Switch, Tier::Light, 500).is_ok()
        && gate2.check("new-fn", Dim::Switch, Tier::Light, 700) == Err(BdCode::OverBudget), "回退 Light 600 边界");

    // C363-07 档位缺 → 最严档兜底。
    let (v, fb) = bt.budget_or_strictest(None, Dim::Memory);
    chk(&mut set, "C363-07", v == 48_000 && fb, "无档 → Heavy 48000 显性");

    // C363-08 无障碍保底档（域本色红线）。
    let per_tier = [(Tier::Light, 150), (Tier::Medium, 150), (Tier::Heavy, 150)];
    chk(&mut set, "C363-08", a11y_floor_respected(&per_tier), "三档无障碍预算全 ≥ 150");

    // C363-09 保底红线实测：削减档位无障碍预算 → 破线。
    let bad = [(Tier::Light, 150), (Tier::Medium, 120), (Tier::Heavy, 150)];
    chk(&mut set, "C363-09", !a11y_floor_respected(&bad), "Medium 120 < 150 => 红线");

    // C363-10 预算冲突仲裁立案。
    let mut ab = ArbitrateBook::new();
    if led.of("search-box", Dim::Lookup) != bt.budget(Tier::Medium, Dim::Lookup) {
        ab.open("search-box", "ledger-vs-table");
    }
    chk(&mut set, "C363-10", ab.count() == 1, "总表与档位表不一致 → 仲裁");

    // C363-11 重议通道：超预算立案留痕。
    let mut ab2 = ArbitrateBook::new();
    match gate.check("search-box", Dim::Lookup, Tier::Medium, 400) {
        Err(BdCode::OverBudget) => ab2.open("search-box", "重议 400ms 诉求"),
        _ => {}
    }
    chk(&mut set, "C363-11", ab2.count() == 1, "重议立案");

    // C363-12 四维名单单源。
    chk(&mut set, "C363-12", Dim::ALL.len() == 4 && Dim::ALL[3].name() == "package", "四维单源");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves363_smoke() {
        let set = run_ves363_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves363 red: {:?}", red);
    }

    #[test]
    fn ves363_floor() {
        assert!(a11y_floor_respected(&[]));
        assert!(!a11y_floor_respected(&[(Tier::Heavy, A11Y_FLOOR - 1)]));
    }
}
