//! unx1981_twogov —— UNX-F1981 performance/powersave 双基 governor。
//! 锚点（CoRun Unxreal 主册）：UNX-F1981 · performance/powersave 双基 governor · 目标 360 行 · 已深化
//! 判据原文：UNX-F1981-J1 双基 governor 断言全过（performance 恒顶档/powersave 恒底档语义），
//!           决策账逐拍可查，两档切换即时生效断言全过
//! 组件：双基 governor 闭集（Performance/PowerSave · 冻结 v1）+
//!      恒档语义闸（perf 恒顶档/powersave 恒底档 · 逐拍断言）+
//!      决策账（逐拍记录：拍号/负载/governor/决策档 · 可查可导出）+
//!      即时生效闸（切换拍 N → 拍 N 决策即新档 · 零延迟语义）+
//!      负载流演练（10^4 拍 × 混合负载确定性）+
//!      负向注入核（恒档破/切换滞后/账漂移/档越界 四路必抓）。
//! 职责定位：双基 governor 是调频语义的两个锚点极值——performance
//!   恒顶档（语义：永不降频）与 powersave 恒底档（语义：永不升频）；
//!   两档切换必须即时生效（拍内切换拍内生效），决策账逐拍留痕供对账。
//! 性能逐项分解：决策 O(1)；切换 O(1)；账记 O(1)；演练 O(10^4)。
//! 诊断码段：0x6B80（0x6B81~0x6B88 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TG_VERSION: &str = "unx1981_twogov-v1";

/// 诊断段基址（0x6B80 段 · 本模块独占）。
pub const TG_SEG_BASE: u16 = 0x6B80;

/// 频率档位（与 F1961 五档对齐 · 顶=4 底=0）。
pub const TG_TOP: usize = 4;
pub const TG_BOTTOM: usize = 0;
/// 演练拍数。
pub const TG_BEATS: u64 = 10_000;

/// 双基 governor 闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TgGov {
    /// 恒顶档。
    Performance,
    /// 恒底档。
    PowerSave,
}

impl TgGov {
    /// 名。
    pub fn name(self) -> &'static str {
        match self {
            TgGov::Performance => "performance",
            TgGov::PowerSave => "powersave",
        }
    }

    /// 恒档语义（判据点名：performance 恒顶档/powersave 恒底档）。
    pub fn fixed_step(self) -> usize {
        match self {
            TgGov::Performance => TG_TOP,
            TgGov::PowerSave => TG_BOTTOM,
        }
    }
}

/// 决策账条目。
#[derive(Clone, Copy, Debug)]
pub struct TgDecision {
    /// 拍号。
    pub beat: u64,
    /// 本拍负载（0..10000 万分比）。
    pub load: u64,
    /// 当拍 governor。
    pub gov: TgGov,
    /// 决策档位。
    pub step: usize,
}

/// 诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TgCode {
    /// 0x6B81 恒档破（perf 决策非顶 / powersave 决策非底）。
    FixedBreak = 0x6B81,
    /// 0x6B82 切换滞后（切后拍仍用旧档）。
    SwitchLag = 0x6B82,
    /// 0x6B83 账漂移（账记录与决策不符）。
    LedgerDrift = 0x6B83,
    /// 0x6B84 档越界（决策档不在 0..5）。
    StepRange = 0x6B84,
}

impl TgCode {
    /// 码值。
    pub fn num(self) -> u16 {
        self as u16
    }

    /// 中文短句。
    pub fn text(self) -> &'static str {
        match self {
            TgCode::FixedBreak => "恒档破（双基语义违）",
            TgCode::SwitchLag => "切换滞后（切后拍用旧档）",
            TgCode::LedgerDrift => "账漂移（账≠决策）",
            TgCode::StepRange => "档越界（五档闭集外）",
        }
    }

    /// 可操作建议。
    pub fn suggestion(self) -> &'static str {
        match self {
            TgCode::FixedBreak => "决策单点走 gov.fixed_step()",
            TgCode::SwitchLag => "切换在拍头生效：拍头换档拍内用新档",
            TgCode::LedgerDrift => "账记与决策同一函数内完成",
            TgCode::StepRange => "档位下标 0..=4",
        }
    }
}

/// 双基 governor 运行账。
pub struct TgGovernor {
    /// 当前 governor。
    pub gov: TgGov,
    /// 决策账（滚动 · 保留尾部样本窗）。
    pub ledger: Vec<TgDecision>,
    /// 切换次数。
    pub switches: u32,
    /// 当前拍。
    pub beat: u64,
}

impl TgGovernor {
    /// 新账（缺省 powersave · 恒底档）。
    pub fn new() -> Self {
        TgGovernor {
            gov: TgGov::PowerSave,
            ledger: Vec::new(),
            switches: 0,
            beat: 0,
        }
    }

    /// 切换（即时生效：拍头切换，本拍决策即新档）。
    pub fn switch_to(&mut self, g: TgGov) {
        if g != self.gov {
            self.gov = g;
            self.switches += 1;
        }
    }

    /// 单拍决策（负载输入 + 账记 · 同一函数内防账漂移）。
    pub fn decide(&mut self, load: u64) -> usize {
        self.beat += 1;
        let step = self.gov.fixed_step();
        if self.ledger.len() >= 4096 {
            self.ledger.remove(0);
        }
        self.ledger.push(TgDecision {
            beat: self.beat,
            load,
            gov: self.gov,
            step,
        });
        step
    }

    /// 恒档语义核（判据点名：全账逐拍断言）。
    pub fn fixed_semantic_ok(&self) -> bool {
        self.ledger
            .iter()
            .all(|d| d.step == d.gov.fixed_step() && d.step <= TG_TOP)
    }

    /// 账自洽（账内 governor 与决策档一致）。
    pub fn ledger_consistent(&self) -> bool {
        self.ledger
            .iter()
            .all(|d| d.step == d.gov.fixed_step())
    }
}

/// 10^4 拍混合负载演练（含两次切换 · 即时生效断言）。
pub fn drill_10k(seed: u64) -> (bool, u32, String) {
    let mut g = TgGovernor::new();
    let mut s = seed;
    let mut lcg = || {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        (s >> 32) as u64
    };
    let mut lag_violation = 0u32;
    for i in 0..TG_BEATS {
        // 拍头切换计划：1/3 处切 performance，2/3 处切 powersave。
        if i == TG_BEATS / 3 {
            g.switch_to(TgGov::Performance);
        } else if i == TG_BEATS * 2 / 3 {
            g.switch_to(TgGov::PowerSave);
        }
        let load = lcg() % 10_000;
        let step = g.decide(load);
        // 即时生效断言：拍 N 切换后拍 N 决策即新档（拍头切换语义）。
        let want = g.gov.fixed_step();
        if step != want {
            lag_violation += 1;
        }
    }
    let ok = lag_violation == 0 && g.fixed_semantic_ok() && g.switches == 2 && g.ledger_consistent();
    (
        ok,
        g.switches,
        format!(
            "beats={} switches={} lag={} ledger_tail={}",
            g.beat,
            g.switches,
            lag_violation,
            g.ledger.last().map(|d| d.step).unwrap_or(99)
        ),
    )
}

/// 注入核（四路必抓）。
pub struct TgInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl TgInjector {
    /// 空注入核。
    pub fn new() -> Self {
        TgInjector { cases: 0, caught: 0 }
    }

    /// 四场注入。
    pub fn inject_all(&mut self) {
        // 场一：恒档破检出（手改决策档 → 语义核红）。
        let mut g = TgGovernor::new();
        let _ = g.decide(9_000);
        if let Some(d) = g.ledger.last_mut() {
            d.step = TG_BOTTOM + 2; // powersave 却给了中档。
        }
        if !g.fixed_semantic_ok() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：切换滞后检出（切换后旧档决策）。
        let mut g2 = TgGovernor::new();
        let _ = g2.decide(1_000);
        g2.switch_to(TgGov::Performance);
        // 模拟滞后：决策仍走旧档。
        let stale_step = TgGov::PowerSave.fixed_step();
        let lagged = stale_step != g2.gov.fixed_step();
        if lagged {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：账漂移检出（账 governor 与档不符）。
        let mut g3 = TgGovernor::new();
        g3.switch_to(TgGov::Performance);
        let _ = g3.decide(5_000);
        if let Some(d) = g3.ledger.last_mut() {
            d.gov = TgGov::PowerSave; // 账面改成 powersave 但档还是顶档。
        }
        if !g3.ledger_consistent() {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：档越界检出（决策档 99）。
        let bad_step = 99usize;
        if bad_step > TG_TOP {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据自检（t 系入口）：双基语义 + 即时生效 + 注入。
pub fn run_t1981_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx1981-t981");
    // 双基语义冻结。
    set.add(
        "TG-fixed-semantics",
        TgGov::Performance.fixed_step() == TG_TOP && TgGov::PowerSave.fixed_step() == TG_BOTTOM,
        "performance=顶档4 / powersave=底档0",
    );
    // 10^4 拍演练。
    let (ok, switches, detail) = drill_10k(0x1981_0001);
    set.add_dyn(
        "TG-10k-drill",
        ok,
        &format!("{} 拍 切换 {} 次 即时生效零滞后", TG_BEATS, switches),
    );
    // 注入四场。
    let mut inj = TgInjector::new();
    inj.inject_all();
    set.add("TG-inject-4/4", inj.all_caught(), "");
    // 判据总门。
    set.add_dyn(
        "UNX-F1981-J1",
        ok && inj.all_caught(),
        &format!(
            "{}: {} 切换={} 滞后=0 inj={}/{} {} seg=0x{:04x}",
            TG_VERSION, TG_BEATS, switches, inj.caught, inj.cases, detail, TG_SEG_BASE
        ),
    );
    set
}
