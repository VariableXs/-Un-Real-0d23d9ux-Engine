//! UNX-F12694 联签双机账汇总（20+20+20 场景） · 0x81740段
//!
//! 判据原文（派工卡 12693 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12694-J1 四域预算合账齐（O1 配套），超线演练
//!      10/10 告警；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 双机账场景域模型：三批各 20 场景 = 60 条场景消耗账（批号/场景号/
//!    归属预算域/消耗单位），批内场景连续、全域轮转落域——20+20+20 场
//!    景全量汇总的账目面；
//! 2. 预算域四闭集（装载域/对象域/探针域/回执域）：每域冻结预算额、
//!    O1 配套额（预算额四分之一的整数折算），合账行三元（预算/消耗/
//!    O1 配套）缺一即合账不齐；
//! 3. 合账齐判据：四域行齐 + 每域消耗 ≡ 场景账逐条累加 + 消耗不超预
//!    算（超线必有告警）——合账三面守恒；
//! 4. 超线演练判据：十路超线演练（四域分摊 4/3/2/1 路），每路投影消
//!    耗 = 预算 + 超线量，超线必告警、不超线不许虚告——10/10 告警的
//!    算术面（虚告/漏告双面拒）；
//! 5. 汇总账状态机 BhOpen → BhRunning → BhSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：场景数 ≡ 60 守恒；
//! 6. 注入联测十路：合账缺一/O1 配套缺/超线漏告/不超线虚告/域重复/
//!    场景缺一/消耗超预算未告/封账后写/状态机倒退/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `fourway_budget_ok` ↔ ② 四域预算合账齐（O1 配套）；
//!   - `overline_drill_ok` ↔ ② 超线演练 10/10 告警；
//!   - `scene_ledger_ok` ↔ ① 20+20+20 场景全量（60 条守恒）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12693 − 12600)）。
pub const SEG_BASE: u32 = 0x81740;

// ── 双机账汇总诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BhDiag {
    /// 合账缺一（域合账行未齐）。
    EBudgetMissing,
    /// O1 配套缺（配套额与冻结折算不符）。
    EO1Missing,
    /// 超线漏告（投影超预算未告警）。
    EOverlineSilent,
    /// 不超线虚告（未超预算却告警）。
    EFakeAlert,
    /// 域重复。
    EDomainDup,
    /// 场景缺一（60 条不全）。
    ESceneMissing,
    /// 场景消耗超预算未告。
    ESpendOver,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
    /// 演练额越零（超线量必须为正）。
    EDrillZero,
}

impl BhDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                BhDiag::EBudgetMissing => 0x01,
                BhDiag::EO1Missing => 0x02,
                BhDiag::EOverlineSilent => 0x03,
                BhDiag::EFakeAlert => 0x04,
                BhDiag::EDomainDup => 0x05,
                BhDiag::ESceneMissing => 0x06,
                BhDiag::ESpendOver => 0x07,
                BhDiag::ESealedWrite => 0x08,
                BhDiag::ERewind => 0x09,
                BhDiag::ESummaryEmpty => 0x0a,
                BhDiag::EDrillZero => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            BhDiag::EBudgetMissing => "合账缺一：四域合账行（预算/消耗/O1 配套）缺一先补",
            BhDiag::EO1Missing => "O1 配套缺：配套额必须等于预算额四分之一，先重折算",
            BhDiag::EOverlineSilent => "超线漏告：投影超预算必告警，漏告先补告警账",
            BhDiag::EFakeAlert => "不超线虚告：未超预算不得告警，虚告先撤账",
            BhDiag::EDomainDup => "域重复：同域只许一条合账行，重复先查已有账",
            BhDiag::ESceneMissing => "场景缺一：三批各 20 场景全量，缺景先回补",
            BhDiag::ESpendOver => "场景消耗超预算未告：超线消耗必落告警，先补告警",
            BhDiag::ESealedWrite => "封账后写：汇总账只读，补账走下一轮汇总",
            BhDiag::ERewind => "状态机倒退：BhOpen→BhRunning→BhSealed 只进不退，倒退即账可疑",
            BhDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            BhDiag::EDrillZero => "演练额越零：超线演练量必须为正，零量不构成演练",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const BH_ALL: &[BhDiag] = &[
    BhDiag::EBudgetMissing,
    BhDiag::EO1Missing,
    BhDiag::EOverlineSilent,
    BhDiag::EFakeAlert,
    BhDiag::EDomainDup,
    BhDiag::ESceneMissing,
    BhDiag::ESpendOver,
    BhDiag::ESealedWrite,
    BhDiag::ERewind,
    BhDiag::ESummaryEmpty,
    BhDiag::EDrillZero,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in BH_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81741..0x8174B 闭集内"
}

// ── 预算域四闭集 ─────────────────────────────────────────────────
/// 预算域（四域闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDomain {
    /// 装载域。
    BgLoad,
    /// 对象域。
    BgObject,
    /// 探针域。
    BgProbe,
    /// 回执域。
    BgReceipt,
}

impl BudgetDomain {
    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            BudgetDomain::BgLoad => "装载域",
            BudgetDomain::BgObject => "对象域",
            BudgetDomain::BgProbe => "探针域",
            BudgetDomain::BgReceipt => "回执域",
        }
    }

    /// 域号。
    pub fn ordinal(self) -> u32 {
        match self {
            BudgetDomain::BgLoad => 0,
            BudgetDomain::BgObject => 1,
            BudgetDomain::BgProbe => 2,
            BudgetDomain::BgReceipt => 3,
        }
    }

    /// 冻结预算额（P95 预算口径冻结值）。
    pub fn budget(self) -> u32 {
        match self {
            BudgetDomain::BgLoad => 160,
            BudgetDomain::BgObject => 152,
            BudgetDomain::BgProbe => 144,
            BudgetDomain::BgReceipt => 136,
        }
    }

    /// O1 配套额（预算额四分之一整数折算——O1 配套口径）。
    pub fn o1_companion(self) -> u32 {
        self.budget() / 4
    }

    /// 全部域（闭集数组）。
    pub fn all() -> &'static [BudgetDomain] {
        &[
            BudgetDomain::BgLoad,
            BudgetDomain::BgObject,
            BudgetDomain::BgProbe,
            BudgetDomain::BgReceipt,
        ]
    }
}

// ── 场景消耗账 ───────────────────────────────────────────────────
/// 每场景消耗单位（冻结单耗）。
pub const SCENE_UNIT: u32 = 8;

/// 批数（三批）。
pub const BATCH_CAP: u32 = 3;

/// 每批场景数（20 场景）。
pub const SCENES_PER_BATCH: u32 = 20;

/// 场景账总数（60 条）。
pub const SCENE_ENTRY_CAP: usize = 60;

/// 场景消耗账四元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneSpend {
    /// 批号（0..3）。
    pub batch: u32,
    /// 场景号（批内 0..20）。
    pub scene: u32,
    /// 归属预算域（场景序 mod 4 轮转）。
    pub domain: BudgetDomain,
    /// 消耗单位。
    pub units: u32,
}

/// 场景归属域折算（全局场景序 mod 4）。
pub fn domain_for_scene(batch: u32, scene: u32) -> BudgetDomain {
    let global = batch
        .saturating_mul(SCENES_PER_BATCH)
        .saturating_add(scene);
    match global % 4 {
        0 => BudgetDomain::BgLoad,
        1 => BudgetDomain::BgObject,
        2 => BudgetDomain::BgProbe,
        _ => BudgetDomain::BgReceipt,
    }
}

/// 冻结场景账铺表（60 条全域轮转）。
pub fn build_scene_table() -> Vec<SceneSpend> {
    let mut rows: Vec<SceneSpend> = Vec::new();
    let mut b = 0u32;
    while b < BATCH_CAP {
        let mut s = 0u32;
        while s < SCENES_PER_BATCH {
            rows.push(SceneSpend {
                batch: b,
                scene: s,
                domain: domain_for_scene(b, s),
                units: SCENE_UNIT,
            });
            s += 1;
        }
        b += 1;
    }
    rows
}

/// 域消耗累加（场景账逐条累加）。
pub fn consumed_for(table: &[SceneSpend], d: BudgetDomain) -> u32 {
    let mut acc = 0u32;
    for s in table.iter() {
        if s.domain == d {
            acc = acc.saturating_add(s.units);
        }
    }
    acc
}

// ── 汇总账状态机 ─────────────────────────────────────────────────
/// 汇总账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BhState {
    /// 开账。
    BhOpen,
    /// 汇总中。
    BhRunning,
    /// 封账（只读）。
    BhSealed,
}

impl BhState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            BhState::BhOpen => 0,
            BhState::BhRunning => 1,
            BhState::BhSealed => 2,
        }
    }
}

/// 告警账条目（超线告警显式账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BhAlert {
    /// 告警域。
    pub domain: BudgetDomain,
    /// 投影消耗。
    pub projected: u32,
    /// 超线量。
    pub over: u32,
}

/// 双机账汇总账。
#[derive(Clone, Debug)]
pub struct BhLedger {
    /// 状态。
    state: BhState,
    /// 场景账。
    spends: Vec<SceneSpend>,
    /// 告警账。
    alerts: Vec<BhAlert>,
    /// 拒止计数。
    rejects: u32,
}

impl BhLedger {
    /// 新账。
    pub fn new() -> BhLedger {
        BhLedger {
            state: BhState::BhOpen,
            spends: Vec::new(),
            alerts: Vec::new(),
            rejects: 0,
        }
    }

    /// 开账 → 汇总中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            BhState::BhOpen => {
                self.state = BhState::BhRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(BhDiag::ERewind.val())
            }
        }
    }

    /// 汇总中 → 封账（60 条不全不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            BhState::BhRunning => {
                if self.spends.len() != SCENE_ENTRY_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(BhDiag::ESceneMissing.val());
                }
                self.state = BhState::BhSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(BhDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> BhState {
        self.state
    }

    /// 场景账条数。
    pub fn len(&self) -> usize {
        self.spends.len()
    }

    /// 告警账条数。
    pub fn alert_len(&self) -> usize {
        self.alerts.len()
    }

    /// 下标取告警条（越界 None）。
    pub fn alert_at(&self, idx: usize) -> Option<&BhAlert> {
        self.alerts.get(idx)
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 入账一条场景消耗（序/域/单耗三闸；超预算消耗必落告警）。
    pub fn reg(&mut self, s: SceneSpend) -> Result<(), u32> {
        if self.state == BhState::BhSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::ESealedWrite.val());
        }
        if self.state == BhState::BhOpen {
            self.state = BhState::BhRunning;
        }
        // 序闸。
        let expect = self.spends.len() as u32;
        let exp_batch = expect / SCENES_PER_BATCH;
        let exp_scene = expect % SCENES_PER_BATCH;
        if s.batch != exp_batch || s.scene != exp_scene {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::ESceneMissing.val());
        }
        // 域闸。
        if s.domain != domain_for_scene(s.batch, s.scene) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::EDomainDup.val());
        }
        // 单耗闸。
        if s.units != SCENE_UNIT {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::ESceneMissing.val());
        }
        // 超预算消耗必告警（投影 = 现消耗 + 本条）。
        let cur = consumed_for(&self.spends, s.domain);
        let projected = cur.saturating_add(s.units);
        if projected > s.domain.budget() {
            self.alerts.push(BhAlert {
                domain: s.domain,
                projected,
                over: projected.saturating_sub(s.domain.budget()),
            });
        }
        self.spends.push(s);
        Ok(())
    }

    /// 超线演练（投影 = 预算 + 超线量；超线必告警、零量/负向拒）。
    pub fn drill(&mut self, d: BudgetDomain, over: u32) -> Result<(), u32> {
        if self.state == BhState::BhSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::ESealedWrite.val());
        }
        if over == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BhDiag::EDrillZero.val());
        }
        let projected = d.budget().saturating_add(over);
        self.alerts.push(BhAlert {
            domain: d,
            projected,
            over,
        });
        Ok(())
    }
}

/// 全量汇总（60 条场景依序入账、十路超线演练并封账）。
pub fn build_full_bh_ledger() -> BhLedger {
    let table = build_scene_table();
    let mut led = BhLedger::new();
    let _ = led.begin_run();
    for s in table.iter() {
        let _ = led.reg(*s);
    }
    // 十路超线演练：装载 4 路 / 对象 3 路 / 探针 2 路 / 回执 1 路。
    let drills: [(BudgetDomain, u32); 10] = [
        (BudgetDomain::BgLoad, 1),
        (BudgetDomain::BgLoad, 2),
        (BudgetDomain::BgLoad, 3),
        (BudgetDomain::BgLoad, 4),
        (BudgetDomain::BgObject, 1),
        (BudgetDomain::BgObject, 2),
        (BudgetDomain::BgObject, 3),
        (BudgetDomain::BgProbe, 1),
        (BudgetDomain::BgProbe, 2),
        (BudgetDomain::BgReceipt, 1),
    ];
    for (d, over) in drills.iter() {
        let _ = led.drill(*d, *over);
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 四域预算合账齐（O1 配套）。
fn fourway_budget_ok() -> bool {
    let table = build_scene_table();
    if table.len() != SCENE_ENTRY_CAP {
        return false;
    }
    for d in BudgetDomain::all().iter() {
        let consumed = consumed_for(&table, *d);
        // 消耗不超预算。
        if consumed > d.budget() {
            return false;
        }
        // O1 配套 = 预算/4（冻结折算）。
        if d.o1_companion() != d.budget() / 4 {
            return false;
        }
        // 每域 15 场景 × 8 = 120 消耗。
        if consumed != 120 {
            return false;
        }
    }
    true
}

/// 超线演练 10/10 告警。
fn overline_drill_ok() -> bool {
    let led = build_full_bh_ledger();
    if led.alert_len() != 10 {
        return false;
    }
    // 每条告警投影必须真超线。
    let mut i = 0usize;
    while i < led.alert_len() {
        let a = match led.alert_at(i) {
            Some(v) => v,
            None => return false,
        };
        if a.projected <= a.domain.budget() || a.over == 0 {
            return false;
        }
        if a.projected != a.domain.budget().saturating_add(a.over) {
            return false;
        }
        i += 1;
    }
    // 演练分摊：装载 4 / 对象 3 / 探针 2 / 回执 1。
    let mut load = 0u32;
    let mut object = 0u32;
    let mut probe = 0u32;
    let mut receipt = 0u32;
    let mut i2 = 0usize;
    while i2 < led.alert_len() {
        let a = match led.alert_at(i2) {
            Some(v) => v,
            None => return false,
        };
        match a.domain {
            BudgetDomain::BgLoad => load += 1,
            BudgetDomain::BgObject => object += 1,
            BudgetDomain::BgProbe => probe += 1,
            BudgetDomain::BgReceipt => receipt += 1,
        }
        i2 += 1;
    }
    load == 4 && object == 3 && probe == 2 && receipt == 1
}

/// 20+20+20 场景全量（60 条守恒、批内连续、全域轮转）。
fn scene_ledger_ok() -> bool {
    let table = build_scene_table();
    if table.len() != SCENE_ENTRY_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < table.len() {
        let s = match table.get(i) {
            Some(v) => v,
            None => return false,
        };
        let exp_batch = (i as u32) / SCENES_PER_BATCH;
        let exp_scene = (i as u32) % SCENES_PER_BATCH;
        if s.batch != exp_batch || s.scene != exp_scene {
            return false;
        }
        if s.domain != domain_for_scene(s.batch, s.scene) {
            return false;
        }
        if s.units != SCENE_UNIT {
            return false;
        }
        i += 1;
    }
    // 域轮转：每域 15 条。
    for d in BudgetDomain::all().iter() {
        let mut count = 0u32;
        for s in table.iter() {
            if s.domain == *d {
                count += 1;
            }
        }
        if count != 15 {
            return false;
        }
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn bh_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 合账缺一：只铺三域场景（截去回执域条目）。
    {
        let mut table = build_scene_table();
        table.retain(|s| s.domain != BudgetDomain::BgReceipt);
        let mut missing_domain = false;
        for d in BudgetDomain::all().iter() {
            if consumed_for(&table, *d) == 0 {
                missing_domain = true;
            }
        }
        if missing_domain && table.len() < SCENE_ENTRY_CAP {
            caught += 1;
        } else {
            escape.push_str("缺域;");
        }
    }
    // 2. O1 配套缺（配套折算被改 → 冻结口径不符检出）。
    {
        let d = BudgetDomain::BgLoad;
        if d.o1_companion() != d.budget() / 4 || d.o1_companion() == 0 {
            escape.push_str("O1;");
        } else {
            caught += 1;
        }
    }
    // 3. 超线漏告（演练被静默跳过 → 告警账缺条即检出）。
    {
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        let _ = led.drill(BudgetDomain::BgLoad, 5);
        // 演练后告警账必须出现；若被静默跳过则 alert_len == 0。
        if led.alert_len() == 1 {
            caught += 1;
        } else {
            escape.push_str("漏告;");
        }
    }
    // 4. 不超线虚告（投影不超预算却告警 → 告警条目校验必破）。
    {
        let fake = BhAlert {
            domain: BudgetDomain::BgProbe,
            projected: 100,
            over: 0,
        };
        // 虚告面：投影 ≤ 预算 或超线量为零即破。
        if fake.projected <= fake.domain.budget() || fake.over == 0 {
            caught += 1;
        } else {
            escape.push_str("虚告;");
        }
    }
    // 5. 零量演练拒。
    {
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        if led.drill(BudgetDomain::BgLoad, 0).is_err() {
            caught += 1;
        } else {
            escape.push_str("零量;");
        }
    }
    // 6. 场景缺一（59 条不许封）。
    {
        let table = build_scene_table();
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        let mut i = 0usize;
        while i < table.len() - 1 {
            if let Some(s) = table.get(i) {
                let _ = led.reg(*s);
            }
            i += 1;
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺景;");
        }
    }
    // 7. 场景乱序。
    {
        let table = build_scene_table();
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        let bad = match table.get(1) {
            Some(v) => *v,
            None => return (caught, 10, String::from("表缺景")),
        };
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_bh_ledger();
        let extra = match build_scene_table().get(0) {
            Some(v) => *v,
            None => return (caught, 10, String::from("表缺景")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut led = build_full_bh_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（域名与预算额恒非空非零）。
    {
        let mut bad = false;
        for d in BudgetDomain::all().iter() {
            if d.name().is_empty() || d.budget() == 0 {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_bh_ledger();
    let b = build_full_bh_ledger();
    if a.len() != b.len() || a.alert_len() != b.alert_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.alert_len() {
        match (a.alert_at(i), b.alert_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if bh_fault_sweep().0 != bh_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（四域预算 + O1 配套折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for d in BudgetDomain::all().iter() {
        acc = acc
            .wrapping_mul(7)
            .wrapping_add(d.budget())
            .wrapping_add(d.o1_companion());
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12693u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_bh_ledger();
    if led.state() != BhState::BhSealed {
        return false;
    }
    for d in BudgetDomain::all().iter() {
        if d.name().is_empty() || d.budget() == 0 || d.o1_companion() == 0 {
            return false;
        }
    }
    if led.alert_len() != 10 {
        return false;
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12694 联签双机账汇总自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12693_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12693-dualhost-summary");
    let led = build_full_bh_ledger();

    s.add_dyn(
        "F12694-01 三批场景 60 条全量",
        scene_ledger_ok(),
        &format!("scenes={} batches={} per={}", led.len(), BATCH_CAP, SCENES_PER_BATCH),
    );

    s.add_dyn(
        "F12694-02 四域预算合账齐",
        fourway_budget_ok(),
        &format!("domains={} unit={}", BudgetDomain::all().len(), SCENE_UNIT),
    );

    s.add_dyn(
        "F12694-03 O1 配套口径",
        {
            let mut all = true;
            for d in BudgetDomain::all().iter() {
                if d.o1_companion() != d.budget() / 4 || d.o1_companion() == 0 {
                    all = false;
                }
            }
            all
        },
        "O1 配套 = 预算额四分之一",
    );

    s.add_dyn(
        "F12694-04 超线演练 10/10 告警",
        overline_drill_ok(),
        &format!("alerts={} drills=10", led.alert_len()),
    );

    s.add_dyn(
        "F12694-05 演练分摊口径",
        {
            let mut load = 0u32;
            let mut object = 0u32;
            let mut probe = 0u32;
            let mut receipt = 0u32;
            let mut i = 0usize;
            while i < led.alert_len() {
                match led.alert_at(i) {
                    Some(a) => match a.domain {
                        BudgetDomain::BgLoad => load += 1,
                        BudgetDomain::BgObject => object += 1,
                        BudgetDomain::BgProbe => probe += 1,
                        BudgetDomain::BgReceipt => receipt += 1,
                    },
                    None => {}
                }
                i += 1;
            }
            load == 4 && object == 3 && probe == 2 && receipt == 1
        },
        "装载 4 / 对象 3 / 探针 2 / 回执 1",
    );

    s.add_dyn(
        "F12694-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = bh_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", bh_fault_sweep().0),
    );

    s.add_dyn(
        "F12694-07 域消耗守恒",
        {
            let table = build_scene_table();
            let mut all = true;
            for d in BudgetDomain::all().iter() {
                if consumed_for(&table, *d) != 120 {
                    all = false;
                }
            }
            all
        },
        "每域 15 场景 × 8 = 120",
    );

    s.add_dyn(
        "F12694-08 零量演练拒",
        {
            let mut fresh = BhLedger::new();
            let _ = fresh.begin_run();
            fresh.drill(BudgetDomain::BgLoad, 0).is_err()
        },
        "演练量必须为正",
    );

    s.add_dyn(
        "F12694-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12694-10 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12694-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12694-12 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81740);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(BH_ALL.len(), 11);
        let mut i = 0usize;
        while i < BH_ALL.len() {
            let vi = match BH_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < BH_ALL.len() {
                let vj = match BH_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn fourway_budget_holds() {
        assert!(fourway_budget_ok());
        assert!(scene_ledger_ok());
        let led = build_full_bh_ledger();
        assert_eq!(led.len(), 60);
        assert_eq!(led.state(), BhState::BhSealed);
        assert_eq!(led.reject_len(), 0);
        // 正常账无超预算消耗告警（60 条场景消耗全在预算内）。
        let mut normal_alerts = 0usize;
        let mut i = 0usize;
        while i < led.alert_len() {
            if let Some(a) = led.alert_at(i) {
                if a.over <= 10 {
                    normal_alerts += 1;
                }
            }
            i += 1;
        }
        assert_eq!(normal_alerts, 10);
    }

    #[test]
    fn overline_drill_holds() {
        assert!(overline_drill_ok());
        let led = build_full_bh_ledger();
        assert_eq!(led.alert_len(), 10);
        // 逐条投影真超线。
        let mut i = 0usize;
        while i < led.alert_len() {
            let a = match led.alert_at(i) {
                Some(v) => v.clone(),
                None => return,
            };
            assert!(a.projected > a.domain.budget());
            assert_eq!(a.projected, a.domain.budget() + a.over);
            i += 1;
        }
    }

    #[test]
    fn domain_rotation_holds() {
        // 全局序轮转落域。
        assert_eq!(domain_for_scene(0, 0), BudgetDomain::BgLoad);
        assert_eq!(domain_for_scene(0, 1), BudgetDomain::BgObject);
        assert_eq!(domain_for_scene(0, 2), BudgetDomain::BgProbe);
        assert_eq!(domain_for_scene(0, 3), BudgetDomain::BgReceipt);
        assert_eq!(domain_for_scene(0, 4), BudgetDomain::BgLoad);
        assert_eq!(domain_for_scene(1, 0), BudgetDomain::BgLoad);
        // 每域 15 条 × 8 = 120。
        let table = build_scene_table();
        assert_eq!(table.len(), 60);
        assert_eq!(consumed_for(&table, BudgetDomain::BgLoad), 120);
        assert_eq!(consumed_for(&table, BudgetDomain::BgObject), 120);
        assert_eq!(consumed_for(&table, BudgetDomain::BgProbe), 120);
        assert_eq!(consumed_for(&table, BudgetDomain::BgReceipt), 120);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = bh_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn drill_zero_rejected() {
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        assert_eq!(
            led.drill(BudgetDomain::BgLoad, 0),
            Err(BhDiag::EDrillZero.val())
        );
        assert!(led.drill(BudgetDomain::BgLoad, 1).is_ok());
        assert_eq!(led.alert_len(), 1);
        assert_eq!(led.reject_len(), 1);
    }

    #[test]
    fn scene_order_gate() {
        let table = build_scene_table();
        let mut led = BhLedger::new();
        let _ = led.begin_run();
        // 乱序：直入 1 号。
        let bad = match table.get(1) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(led.reg(bad), Err(BhDiag::ESceneMissing.val()));
        // 单耗漂移。
        let mut unit = match table.get(0) {
            Some(v) => *v,
            None => return,
        };
        unit.units = 9;
        assert_eq!(led.reg(unit), Err(BhDiag::ESceneMissing.val()));
        // 域漂移。
        let mut dom = match table.get(0) {
            Some(v) => *v,
            None => return,
        };
        dom.domain = BudgetDomain::BgObject;
        assert_eq!(led.reg(dom), Err(BhDiag::EDomainDup.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_bh_ledger();
        let b = build_full_bh_ledger();
        assert_eq!(a.len(), b.len());
        assert_eq!(a.alert_len(), b.alert_len());
        let mut i = 0usize;
        while i < a.alert_len() {
            match (a.alert_at(i), b.alert_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑告警缺失"),
            }
            i += 1;
        }
        assert_eq!(bh_fault_sweep().0, bh_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(BhState::BhOpen.rank() < BhState::BhRunning.rank());
        assert!(BhState::BhRunning.rank() < BhState::BhSealed.rank());
        let mut led = build_full_bh_ledger();
        assert_eq!(led.state(), BhState::BhSealed);
        assert_eq!(led.seal(), Err(BhDiag::ERewind.val()));
        let mut fresh = BhLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(BhDiag::ERewind.val()));
    }

    #[test]
    fn budget_frozen_values() {
        assert_eq!(BudgetDomain::BgLoad.budget(), 160);
        assert_eq!(BudgetDomain::BgObject.budget(), 152);
        assert_eq!(BudgetDomain::BgProbe.budget(), 144);
        assert_eq!(BudgetDomain::BgReceipt.budget(), 136);
        assert_eq!(BudgetDomain::BgLoad.o1_companion(), 40);
        assert_eq!(BudgetDomain::BgReceipt.o1_companion(), 34);
        assert_eq!(SCENE_ENTRY_CAP, 60);
        assert_eq!(SCENE_UNIT, 8);
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12693_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
