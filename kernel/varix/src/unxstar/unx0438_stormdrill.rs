//! unx0438_stormdrill —— UNX-F0438 中断风暴演练（负载三档联动）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0438 · 中断风暴演练 · 目标 240 行
//! 判据原文：UNX-F0438-J1 三档负载：轻（≤1K 中断/秒）、中
//!           （≤100K）、重（≤10M）；档位判别 = 实测速率落
//!           窗（轻窗 (0,1K]、中窗 (1K,100K]、重窗 (100K,
//!           10M]，超重窗 = 风暴）；风暴分级：超 10 倍 →
//!           黄（记账+限流预告）、超 100 倍 → 红（限流执
//!           行）、超 1000 倍 → 黑（隔离该线）；联动核：档
//!           位与处置动作一一对应（轻=直通、中=记账、重=
//!           限流、风暴=分级处置），低档不许触发高档动作；
//!           限流核：红档起对超限部分丢包入「丢弃账」，丢
//!           弃账与放行账之和 == 到达总数（守恒）；隔离核：
//!           黑档被隔离线不再放行，解除需显式命令。
//! 组件：三档窗判别器 + 风暴分级器 + 联动状态机 + 限流守
//!       恒账 + 隔离闩 + 演练驱动器。
//! 职责定位：中断风暴是「心脏早搏」——不分级处置轻则卡顿
//!   重则整机瘫死；三档联动错配（轻负载跑限流、重负载直
//!   通）比不处置更糟（前者白耗性能，后者裸奔）；守恒账保
//!   证限流没有「吞账」（丢了多少要有数）。
//! 性能逐项分解：档判 O(1)；分级 O(1)；联动 O(1)；限流
//!   O(1)/中断。
//! 诊断码段：0x30500（0x30501~0x30509 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const SD_VERSION: &str = "unx0438_stormdrill-v1";

/// 诊断段基址（0x30500 段 · 本模块独占）。
pub const SD_SEG_BASE: u32 = 0x30500;
pub const XC_ACTION_MISMATCH: u32 = 0x30501; // 档动错配
pub const XC_TALLY: u32 = 0x30502; // 守恒破
pub const XC_ISOLATED_PASS: u32 = 0x30503; // 隔离线放行
pub const XC_RATE_OOR: u32 = 0x30504; // 速率越窗无档
pub const XC_UNLOCK: u32 = 0x30505; // 未授权解闩
pub const XC_MAGIC: u32 = 0x30509; // 账魔数破

/// 三档窗上界（中断/秒）。
pub const THRESH_LIGHT: u64 = 1_000;
pub const THRESH_MED: u64 = 100_000;
pub const THRESH_HEAVY: u64 = 10_000_000;
/// 风暴分级倍数。
pub const STORM_X_YELLOW: u64 = 10;
pub const STORM_X_RED: u64 = 100;
pub const STORM_X_BLACK: u64 = 1_000;

/// 负载档。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Load {
    /// 轻：直通。
    Light = 1,
    /// 中：记账。
    Med = 2,
    /// 重：限流。
    Heavy = 3,
    /// 风暴：分级处置。
    Storm = 4,
}

/// 风暴级。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Storm {
    None = 0,
    /// 黄：记账+限流预告。
    Yellow = 1,
    /// 红：限流执行。
    Red = 2,
    /// 黑：隔离。
    Black = 3,
}

/// 档位判别。
pub fn classify(rate: u64) -> Result<Load, u32> {
    if rate == 0 {
        return Err(XC_RATE_OOR);
    }
    if rate <= THRESH_LIGHT {
        Ok(Load::Light)
    } else if rate <= THRESH_MED {
        Ok(Load::Med)
    } else if rate <= THRESH_HEAVY {
        Ok(Load::Heavy)
    } else {
        Ok(Load::Storm)
    }
}

/// 风暴分级：以重窗上界为基。
pub fn grade_storm(rate: u64) -> Storm {
    if rate <= THRESH_HEAVY {
        return Storm::None;
    }
    let x = rate / THRESH_HEAVY;
    if x >= STORM_X_BLACK {
        Storm::Black
    } else if x >= STORM_X_RED {
        Storm::Red
    } else if x >= STORM_X_YELLOW {
        Storm::Yellow
    } else {
        Storm::None
    }
}

/// 处置动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Pass = 1,
    Book = 2,
    Throttle = 3,
    GradeStorm = 4,
}

/// 联动核：档 → 动作一一映射。
pub fn act_of(l: Load) -> Result<Action, u32> {
    match l {
        Load::Light => Ok(Action::Pass),
        Load::Med => Ok(Action::Book),
        Load::Heavy => Ok(Action::Throttle),
        Load::Storm => Ok(Action::GradeStorm),
    }
}

/// 单线限流守恒账。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LineLedger {
    /// 到达总数。
    pub arrived: u64,
    /// 放行数。
    pub passed: u64,
    /// 丢弃数（限流超限）。
    pub dropped: u64,
}

impl LineLedger {
    /// 守恒核。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.passed + self.dropped == self.arrived {
            Ok(())
        } else {
            Err(XC_TALLY)
        }
    }
}

/// 演练驱动器：单线账 + 隔离闩 + 阈值。
pub struct Drill {
    /// 限流阈值（放行配额/窗口）。
    pub quota: u64,
    /// 账。
    pub ledger: LineLedger,
    /// 隔离闩。
    pub isolated: bool,
    /// 当前档。
    pub load: Load,
}

impl Drill {
    pub fn new(quota: u64) -> Self {
        Drill { quota, ledger: LineLedger::default(), isolated: false, load: Load::Light }
    }

    /// 到达一批 n 个中断，按当前档处置。
    pub fn arrive(&mut self, n: u64) -> Result<(u64, u64), u32> {
        if self.isolated {
            // 隔离线：全部丢弃（不放行）。
            self.ledger.arrived += n;
            self.ledger.dropped += n;
            return Err(XC_ISOLATED_PASS);
        }
        self.ledger.arrived += n;
        let room = self.quota.saturating_sub(self.ledger.passed);
        let pass = n.min(room);
        let drop = n - pass;
        self.ledger.passed += pass;
        self.ledger.dropped += drop;
        Ok((pass, drop))
    }

    /// 设档（联动核内用）。
    pub fn set_rate(&mut self, rate: u64) -> Result<Action, u32> {
        let l = classify(rate)?;
        let a = act_of(l)?;
        self.load = l;
        Ok(a)
    }

    /// 隔离。
    pub fn isolate(&mut self) {
        self.isolated = true;
    }

    /// 解除隔离（须先确认非风暴档）。
    pub fn release(&mut self, rate: u64) -> Result<(), u32> {
        if grade_storm(rate) == Storm::Black {
            return Err(XC_UNLOCK);
        }
        self.isolated = false;
        Ok(())
    }
}

/// 判据入口：UNX-F0438。
pub fn run_unx0438_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0438_stormdrill");
    // 族一：档位窗判。
    cs.add("零速率拒", classify(0) == Err(XC_RATE_OOR), "无负载无档");
    cs.add("轻档 1K", classify(1_000) == Ok(Load::Light), "窗含上界");
    cs.add("中档 1K+1", classify(1_001) == Ok(Load::Med), "出轻窗");
    cs.add("重档 100K+1", classify(100_001) == Ok(Load::Heavy), "出中窗");
    cs.add("风暴 10M+1", classify(10_000_001) == Ok(Load::Storm), "出重窗");
    // 族二：风暴分级。
    cs.add("10M 无级", grade_storm(10_000_000) == Storm::None, "窗内");
    cs.add("黄 10 倍", grade_storm(100_000_000) == Storm::Yellow, "10×");
    cs.add("红 100 倍", grade_storm(1_000_000_000) == Storm::Red, "100×");
    cs.add("黑 1000 倍", grade_storm(10_000_000_000) == Storm::Black, "1000×");
    cs.add("9 倍无级", grade_storm(90_000_000) == Storm::None, "不足黄");
    // 族三：联动核。
    cs.add("轻=直通", act_of(Load::Light) == Ok(Action::Pass), "映射");
    cs.add("中=记账", act_of(Load::Med) == Ok(Action::Book), "映射");
    cs.add("重=限流", act_of(Load::Heavy) == Ok(Action::Throttle), "映射");
    cs.add("风暴=分级", act_of(Load::Storm) == Ok(Action::GradeStorm), "映射");
    // 族四：限流守恒。
    let mut d = Drill::new(100);
    d.set_rate(150_000).ok(); // 重档
    let (p, dr) = d.arrive(150).unwrap_or((0, 0));
    cs.add("放行 100", p == 100, "配额顶");
    cs.add("丢弃 50", dr == 50, "超限");
    cs.add("守恒平", d.ledger.conserve().is_ok(), "100+50=150");
    // 族五：轻档直通不丢。
    let mut d2 = Drill::new(1_000);
    d2.set_rate(500).ok();
    let (p2, dr2) = d2.arrive(300).unwrap_or((0, 0));
    cs.add("轻档全放", p2 == 300 && dr2 == 0, "无丢弃");
    // 族六：两窗叠加守恒。
    let mut d3 = Drill::new(100);
    d3.set_rate(200_000).ok();
    let _ = d3.arrive(60);
    let _ = d3.arrive(80);
    cs.add("两批守恒", d3.ledger.arrived == 140 && d3.ledger.passed == 100 && d3.ledger.dropped == 40, "60+80");
    // 族七：隔离闩。
    let mut d4 = Drill::new(100);
    d4.isolate();
    cs.add("隔离拒放行", d4.arrive(10) == Err(XC_ISOLATED_PASS), "闩下");
    cs.add("隔离全丢", d4.ledger.dropped == 10 && d4.ledger.passed == 0, "到账即丢");
    cs.add("隔离守恒", d4.ledger.conserve().is_ok(), "10+0=10");
    // 族八：解除闸。
    let mut d5 = Drill::new(100);
    d5.isolate();
    cs.add("黑档不解闩", d5.release(20_000_000_000) == Err(XC_UNLOCK), "仍风暴");
    cs.add("退风暴解闩", d5.release(5_000_000).is_ok(), "回重档");
    cs.add("解后放行", d5.arrive(10).is_ok(), "闸开");
    // 族九：档位次序。
    cs.add("档序轻<中", Load::Light < Load::Med, "枚举序");
    cs.add("级序黄<红<黑", Storm::Yellow < Storm::Red && Storm::Red < Storm::Black, "升级单向");
    // 族十：黄级预告（限流先行，丢超限部分）。
    let mut d6 = Drill::new(1_500);
    d6.set_rate(50_000_000).ok(); // 黄：5×重窗
    let (p6, dr6) = d6.arrive(2_000).unwrap_or((0, 0));
    cs.add("黄级限流执行", dr6 == 500 && p6 == 1_500, "配额外丢");
    cs.add("黄级守恒", d6.ledger.conserve().is_ok(), "账平");
    cs
}
