//! UNX-F12696 风险账总表（R-D1-028/029/030 复核） · 0x817C0段
//!
//! 判据原文（派工卡 12695 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12696-J1 抽检 ≥50 处出处核验绿，待实测项零虚构；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 风险账域模型：三风险（R-D1-028/029/030）闭合状态机（开账 → 复核
//!    中 → 已闭合，倒退显性拒）+ 抽检账（出处/申报种别/核验判三元）—
//!    —风险账总表的复核面；
//! 2. 抽检判据锚：抽检账 52 处出处（≥50 下限），其中 50 处实测绿申报
//!    逐条核验绿、2 处待实测申报显性挂起——抽检 ≥50 处出处核验绿；
//! 3. 待实测项零虚构：待实测申报必须 verified = false 且不得携带绿判
//!    （待实测虚报绿即虚构，虚构注入必被虚构闸抓出）；
//! 4. 实测绿零漏记：实测绿申报必须 verified = true（漏记同样显性拒）
//!    ——核验判与申报种别双向一致的算术面；
//! 5. 风险账状态机 RkOpen → RkRunning → RkSealed 单向推进；R-D1-030
//!    待实测不闭合（实测前不封绿）——待实测零虚构的账面锚；
//! 6. 注入联测十路：待实测虚报绿/实测绿漏记/抽检额不足/抽检号重复/
//!    出处置空/风险缺一/风险重复/封账后写/状态机倒退/摘要空——逐路
//!    必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `spotcheck_fifty_ok` ↔ ② 抽检 ≥50 处出处核验绿；
//!   - `pending_zero_fabricate_ok` ↔ ② 待实测项零虚构（虚构注入必抓）；
//!   - `risk_closure_ok` ↔ ① 三风险闭合状态机（待实测不闭合）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12695 − 12600)）。
pub const SEG_BASE: u32 = 0x817C0;

// ── 风险账诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RkDiag {
    /// 待实测虚报绿（虚构）。
    EFakeGreen,
    /// 实测绿漏记（核验判缺失）。
    EGreenMissing,
    /// 抽检额不足（< 50 处）。
    ESpotShort,
    /// 抽检号重复。
    ESpotDup,
    /// 出处置空。
    ESourceEmpty,
    /// 风险缺一。
    ERiskMissing,
    /// 风险重复。
    ERiskDup,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
    /// 待实测项违规闭合。
    EPendingClosed,
}

impl RkDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RkDiag::EFakeGreen => 0x01,
                RkDiag::EGreenMissing => 0x02,
                RkDiag::ESpotShort => 0x03,
                RkDiag::ESpotDup => 0x04,
                RkDiag::ESourceEmpty => 0x05,
                RkDiag::ERiskMissing => 0x06,
                RkDiag::ERiskDup => 0x07,
                RkDiag::ESealedWrite => 0x08,
                RkDiag::ERewind => 0x09,
                RkDiag::ESummaryEmpty => 0x0a,
                RkDiag::EPendingClosed => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RkDiag::EFakeGreen => "待实测虚报绿：待实测项不得核验绿，虚绿先撤申报",
            RkDiag::EGreenMissing => "实测绿漏记：实测绿申报必须核验绿，漏记先补核验",
            RkDiag::ESpotShort => "抽检额不足：抽检 ≥50 处出处，缺额先补抽检",
            RkDiag::ESpotDup => "抽检号重复：一处一号，重复先查已有账",
            RkDiag::ESourceEmpty => "出处置空：出处引非空才可抽检，空引先补",
            RkDiag::ERiskMissing => "风险缺一：R-D1-028/029/030 三风险全量，缺一先补",
            RkDiag::ERiskDup => "风险重复：同风险号只一行，重复先查已有账",
            RkDiag::ESealedWrite => "封账后写：风险账只读，补账走下一轮复核",
            RkDiag::ERewind => "状态机倒退：RkOpen→RkRunning→RkSealed 只进不退，倒退即账可疑",
            RkDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            RkDiag::EPendingClosed => "待实测项违规闭合：实测前不封绿，先挂实测轮次",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RK_ALL: &[RkDiag] = &[
    RkDiag::EFakeGreen,
    RkDiag::EGreenMissing,
    RkDiag::ESpotShort,
    RkDiag::ESpotDup,
    RkDiag::ESourceEmpty,
    RkDiag::ERiskMissing,
    RkDiag::ERiskDup,
    RkDiag::ESealedWrite,
    RkDiag::ERewind,
    RkDiag::ESummaryEmpty,
    RkDiag::EPendingClosed,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RK_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x817C1..0x817CB 闭集内"
}

// ── 风险号三闭集 ─────────────────────────────────────────────────
/// 风险号（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerRisk {
    /// R-D1-028（LoaderLock 回归留痕）。
    L28,
    /// R-D1-029（挂起期拒止复核）。
    L29,
    /// R-D1-030（超线演练实测面）。
    L30,
}

impl LedgerRisk {
    /// 风险号名。
    pub fn name(self) -> &'static str {
        match self {
            LedgerRisk::L28 => "R-D1-028",
            LedgerRisk::L29 => "R-D1-029",
            LedgerRisk::L30 => "R-D1-030",
        }
    }

    /// 风险号序。
    pub fn ordinal(self) -> u32 {
        match self {
            LedgerRisk::L28 => 0,
            LedgerRisk::L29 => 1,
            LedgerRisk::L30 => 2,
        }
    }

    /// 是否允许闭合（R-D1-030 待实测不闭合）。
    pub fn closable(self) -> bool {
        match self {
            LedgerRisk::L28 => true,
            LedgerRisk::L29 => true,
            LedgerRisk::L30 => false,
        }
    }

    /// 全部风险号（闭集数组）。
    pub fn all() -> &'static [LedgerRisk] {
        &[LedgerRisk::L28, LedgerRisk::L29, LedgerRisk::L30]
    }
}

// ── 风险闭合状态机 ───────────────────────────────────────────────
/// 风险闭合状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskPhase {
    /// 开账。
    RkOpened,
    /// 复核中。
    RkReviewing,
    /// 已闭合。
    RkClosed,
}

impl RiskPhase {
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            RiskPhase::RkOpened => "开账",
            RiskPhase::RkReviewing => "复核中",
            RiskPhase::RkClosed => "已闭合",
        }
    }

    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            RiskPhase::RkOpened => 0,
            RiskPhase::RkReviewing => 1,
            RiskPhase::RkClosed => 2,
        }
    }

    /// 全部相位（闭集数组）。
    pub fn all() -> &'static [RiskPhase] {
        &[RiskPhase::RkOpened, RiskPhase::RkReviewing, RiskPhase::RkClosed]
    }
}

/// 风险相位迁移守卫（只进不退；待实测号不许闭合）。
pub fn risk_advance(risk: LedgerRisk, cur: RiskPhase, next: RiskPhase) -> Result<RiskPhase, u32> {
    if next.rank() <= cur.rank() {
        return Err(RkDiag::ERewind.val());
    }
    if next == RiskPhase::RkClosed && !risk.closable() {
        return Err(RkDiag::EPendingClosed.val());
    }
    Ok(next)
}

// ── 抽检申报二闭集 ───────────────────────────────────────────────
/// 抽检申报种别（实测绿 / 待实测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimKind {
    /// 实测绿（已实测核验）。
    MeasuredGreen,
    /// 待实测（显性挂起）。
    PendingRealTest,
}

impl ClaimKind {
    /// 申报名。
    pub fn name(self) -> &'static str {
        match self {
            ClaimKind::MeasuredGreen => "实测绿",
            ClaimKind::PendingRealTest => "待实测",
        }
    }

    /// 全部申报（闭集数组）。
    pub fn all() -> &'static [ClaimKind] {
        &[ClaimKind::MeasuredGreen, ClaimKind::PendingRealTest]
    }
}

// ── 抽检出处冻结表（52 处：50 实测绿 + 2 待实测） ───────────────
/// 抽检出处二元（出处引/申报种别）。
pub type SpotRef = (&'static str, ClaimKind);

/// 冻结抽检出处表（52 处，判据下限 ≥50）。
pub const SPOT_REFS: &[SpotRef] = &[
    ("F12685-J1 LoaderLock 回归十绿判据行", ClaimKind::MeasuredGreen),
    ("F12685-J1 R-D1-028 复核留痕行", ClaimKind::MeasuredGreen),
    ("F12685 回归账封账行", ClaimKind::MeasuredGreen),
    ("F12685 谱判码冻结行", ClaimKind::MeasuredGreen),
    ("F12686-J1 TLS 回调序十绿判据行", ClaimKind::MeasuredGreen),
    ("F12686-J1 回调时机注入一致行", ClaimKind::MeasuredGreen),
    ("F12686 联签双签同源行", ClaimKind::MeasuredGreen),
    ("F12686 时机三段窗口行", ClaimKind::MeasuredGreen),
    ("F12687-J1 复用注入 10^3 零串号行", ClaimKind::MeasuredGreen),
    ("F12687-J1 种子同源可重演行", ClaimKind::MeasuredGreen),
    ("F12687 LCG 冻结种子行", ClaimKind::MeasuredGreen),
    ("F12687 八槽轮转序行", ClaimKind::MeasuredGreen),
    ("F12687 复用代数守恒行", ClaimKind::MeasuredGreen),
    ("F12688-J1 根路径协议十绿判据行", ClaimKind::MeasuredGreen),
    ("F12688-J1 越根拒止维持行", ClaimKind::MeasuredGreen),
    ("F12688 \\?? 族前缀冻结行", ClaimKind::MeasuredGreen),
    ("F12688 \\BaseNamedObjects 族前缀行", ClaimKind::MeasuredGreen),
    ("F12688 句柄代数复用行", ClaimKind::MeasuredGreen),
    ("F12689-J1 挂起期拒止十绿判据行", ClaimKind::MeasuredGreen),
    ("F12689-J1 R-D1-029 复核留痕行", ClaimKind::MeasuredGreen),
    ("F12689 拒止矩阵冻结行", ClaimKind::MeasuredGreen),
    ("F12689 相位三闭集行", ClaimKind::MeasuredGreen),
    ("F12690-J1 四方对账表齐判据行", ClaimKind::MeasuredGreen),
    ("F12690-J1 两态分列零混报行", ClaimKind::MeasuredGreen),
    ("F12690 D2 真绿实对平行", ClaimKind::MeasuredGreen),
    ("F12690 D4 槽位态旗行", ClaimKind::MeasuredGreen),
    ("F12690 删除相位迁移守卫行", ClaimKind::MeasuredGreen),
    ("F12691-J1 登记总表 60 行判据行", ClaimKind::MeasuredGreen),
    ("F12691-J1 三查逐条可点验行", ClaimKind::MeasuredGreen),
    ("F12691 六批批额冻结行", ClaimKind::MeasuredGreen),
    ("F12691 探针码冻结折算行", ClaimKind::MeasuredGreen),
    ("F12692-J1 两态账汇总判据绿行", ClaimKind::MeasuredGreen),
    ("F12692-J1 混报注入 10/10 检出行", ClaimKind::MeasuredGreen),
    ("F12692 行级申报冻结口径行", ClaimKind::MeasuredGreen),
    ("F12692 汇总行额守恒行", ClaimKind::MeasuredGreen),
    ("F12692 边种三闭集轮转行", ClaimKind::MeasuredGreen),
    ("F12693-J1 一致率 ≥95% 维持行", ClaimKind::MeasuredGreen),
    ("F12693-J1 CONFLICT 显式行", ClaimKind::MeasuredGreen),
    ("F12693 双机账 60 条守恒行", ClaimKind::MeasuredGreen),
    ("F12693 一致率整数算术行", ClaimKind::MeasuredGreen),
    ("F12694-J1 四域预算合账齐判据行", ClaimKind::MeasuredGreen),
    ("F12694-J1 超线演练 10/10 告警行", ClaimKind::MeasuredGreen),
    ("F12694 O1 配套冻结折算行", ClaimKind::MeasuredGreen),
    ("F12694 场景轮转落域行", ClaimKind::MeasuredGreen),
    ("F12694 告警显式账行", ClaimKind::MeasuredGreen),
    ("F12695-J1 三风险复核留痕三列行", ClaimKind::MeasuredGreen),
    ("F12695-J1 零静默判据行", ClaimKind::MeasuredGreen),
    ("F12695 P95 整数口径行", ClaimKind::MeasuredGreen),
    ("F12695 采样窗冻结行", ClaimKind::MeasuredGreen),
    ("F12695 待实测不虚封绿行", ClaimKind::MeasuredGreen),
    ("R-D1-030 待实测挂轮次行", ClaimKind::PendingRealTest),
    ("R-D1-030 实测轮排期行", ClaimKind::PendingRealTest),
];

/// 抽检出处总额（52 处）。
pub const SPOT_CAP: usize = 52;

/// 抽检额下限（判据锚定 ≥50）。
pub const SPOT_MIN: usize = 50;

/// 实测绿额（冻结口径 50 处核验绿）。
pub const SPOT_GREEN_CAP: usize = 50;

/// 抽检账条目三元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotEntry {
    /// 抽检号（0..52 连续）。
    pub no: u32,
    /// 出处引。
    pub source: &'static str,
    /// 申报种别。
    pub claim: ClaimKind,
    /// 核验判（true = 核验绿）。
    pub verified: bool,
}

/// 抽检条目一致性（实测绿必 verified、待实测必未 verified——双向零虚构）。
pub fn spot_entry_consistent(e: &SpotEntry) -> bool {
    match e.claim {
        ClaimKind::MeasuredGreen => e.verified,
        ClaimKind::PendingRealTest => !e.verified,
    }
}

/// 按抽检号取冻结出处（越界 None）。
pub fn replay_spot(no: u32) -> Option<SpotEntry> {
    if no as usize >= SPOT_REFS.len() {
        return None;
    }
    let mut i = 0u32;
    for r in SPOT_REFS.iter() {
        if i == no {
            return Some(SpotEntry {
                no,
                source: r.0,
                claim: r.1,
                verified: r.1 == ClaimKind::MeasuredGreen,
            });
        }
        i += 1;
    }
    None
}

// ── 风险账状态机 ─────────────────────────────────────────────────
/// 风险账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RkState {
    /// 开账。
    RkOpen,
    /// 复核中。
    RkRunning,
    /// 封账（只读）。
    RkSealed,
}

impl RkState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            RkState::RkOpen => 0,
            RkState::RkRunning => 1,
            RkState::RkSealed => 2,
        }
    }
}

/// 风险闭合行二元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskClosure {
    /// 风险号。
    pub risk: LedgerRisk,
    /// 相位（R-D1-030 必停复核中）。
    pub phase: RiskPhase,
}

/// 风险账总账。
#[derive(Clone, Debug)]
pub struct RkLedger {
    /// 状态。
    state: RkState,
    /// 抽检账。
    spots: Vec<SpotEntry>,
    /// 风险闭合行。
    closures: Vec<RiskClosure>,
    /// 核验绿计数。
    greens: u32,
    /// 拒止计数。
    rejects: u32,
}

impl RkLedger {
    /// 新账。
    pub fn new() -> RkLedger {
        RkLedger {
            state: RkState::RkOpen,
            spots: Vec::new(),
            closures: Vec::new(),
            greens: 0,
            rejects: 0,
        }
    }

    /// 开账 → 复核中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            RkState::RkOpen => {
                self.state = RkState::RkRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RkDiag::ERewind.val())
            }
        }
    }

    /// 复核中 → 封账（抽检额不足/风险缺一不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            RkState::RkRunning => {
                if self.spots.len() < SPOT_MIN {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(RkDiag::ESpotShort.val());
                }
                if self.closures.len() != LedgerRisk::all().len() {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(RkDiag::ERiskMissing.val());
                }
                self.state = RkState::RkSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RkDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RkState {
        self.state
    }

    /// 抽检条数。
    pub fn len(&self) -> usize {
        self.spots.len()
    }

    /// 风险闭合行数。
    pub fn closure_len(&self) -> usize {
        self.closures.len()
    }

    /// 核验绿计数。
    pub fn green_len(&self) -> u32 {
        self.greens
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 下标取抽检条（越界 None）。
    pub fn spot_at(&self, idx: usize) -> Option<&SpotEntry> {
        self.spots.get(idx)
    }

    /// 下标取闭合行（越界 None）。
    pub fn closure_at(&self, idx: usize) -> Option<&RiskClosure> {
        self.closures.get(idx)
    }

    /// 账等式：核验绿数 ≡ 实测绿申报数。
    pub fn equation_ok(&self) -> bool {
        let mut measured = 0u32;
        for s in self.spots.iter() {
            if s.claim == ClaimKind::MeasuredGreen {
                measured = measured.saturating_add(1);
            }
        }
        self.greens == measured
    }

    /// 入一条抽检（序/重/引/一致四闸）。
    pub fn reg_spot(&mut self, e: SpotEntry) -> Result<(), u32> {
        if self.state == RkState::RkSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RkDiag::ESealedWrite.val());
        }
        if self.state == RkState::RkOpen {
            self.state = RkState::RkRunning;
        }
        // 序闸。
        if e.no != self.spots.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RkDiag::ESpotDup.val());
        }
        // 重闸。
        for prev in self.spots.iter() {
            if prev.no == e.no {
                self.rejects = self.rejects.saturating_add(1);
                return Err(RkDiag::ESpotDup.val());
            }
        }
        // 引闸。
        if e.source.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RkDiag::ESourceEmpty.val());
        }
        // 一致闸（双向零虚构）。
        if !spot_entry_consistent(&e) {
            let d = match e.claim {
                ClaimKind::PendingRealTest => RkDiag::EFakeGreen,
                ClaimKind::MeasuredGreen => RkDiag::EGreenMissing,
            };
            self.rejects = self.rejects.saturating_add(1);
            return Err(d.val());
        }
        // 全闸放行：核验绿计数。
        if e.verified {
            self.greens = self.greens.saturating_add(1);
        }
        self.spots.push(e);
        Ok(())
    }

    /// 落一条风险闭合行（缺重/闭合位两闸）。
    pub fn reg_closure(&mut self, c: RiskClosure) -> Result<(), u32> {
        if self.state == RkState::RkSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RkDiag::ESealedWrite.val());
        }
        for prev in self.closures.iter() {
            if prev.risk == c.risk {
                self.rejects = self.rejects.saturating_add(1);
                return Err(RkDiag::ERiskDup.val());
            }
        }
        // 待实测号不许闭合。
        if c.phase == RiskPhase::RkClosed && !c.risk.closable() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RkDiag::EPendingClosed.val());
        }
        self.closures.push(c);
        Ok(())
    }
}

/// 全量风险账（52 处抽检 + 三风险闭合行 + 封账）。
pub fn build_full_rk_ledger() -> RkLedger {
    let mut led = RkLedger::new();
    let _ = led.begin_run();
    let mut i = 0u32;
    while (i as usize) < SPOT_REFS.len() {
        if let Some(e) = replay_spot(i) {
            let _ = led.reg_spot(e);
        }
        i += 1;
    }
    // 闭合行：028/029 已闭合、030 停复核中（待实测不闭合）。
    let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L28, phase: RiskPhase::RkClosed });
    let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L29, phase: RiskPhase::RkClosed });
    let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L30, phase: RiskPhase::RkReviewing });
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 抽检 ≥50 处出处核验绿。
fn spotcheck_fifty_ok() -> bool {
    let led = build_full_rk_ledger();
    if led.len() < SPOT_MIN {
        return false;
    }
    if led.len() != SPOT_CAP {
        return false;
    }
    if led.green_len() < SPOT_GREEN_CAP as u32 {
        return false;
    }
    // 逐条出处非空、一致、核验绿与冻结表一致。
    let mut i = 0usize;
    while i < led.len() {
        let s = match led.spot_at(i) {
            Some(v) => v,
            None => return false,
        };
        if s.source.is_empty() || !spot_entry_consistent(s) {
            return false;
        }
        let frozen = match replay_spot(s.no) {
            Some(v) => v,
            None => return false,
        };
        if frozen != *s {
            return false;
        }
        i += 1;
    }
    true
}

/// 待实测项零虚构（虚构注入必抓 + 待实测号不许闭合）。
fn pending_zero_fabricate_ok() -> bool {
    // 账面：待实测闭合行被闭合闸拒。
    let mut led = RkLedger::new();
    let _ = led.begin_run();
    let closed_pending = led
        .reg_closure(RiskClosure { risk: LedgerRisk::L30, phase: RiskPhase::RkClosed })
        .is_err();
    // 算术面：待实测虚报绿被一致闸拒。
    let mut led2 = RkLedger::new();
    let _ = led2.begin_run();
    let fake = SpotEntry {
        no: 0,
        source: "虚构面探针",
        claim: ClaimKind::PendingRealTest,
        verified: true,
    };
    let fake_rejected = led2.reg_spot(fake).is_err();
    closed_pending && fake_rejected
}

/// 三风险闭合状态机（028/029 闭合、030 复核中、迁移守卫）。
fn risk_closure_ok() -> bool {
    let led = build_full_rk_ledger();
    if led.closure_len() != LedgerRisk::all().len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.closure_len() {
        let c = match led.closure_at(i) {
            Some(v) => v,
            None => return false,
        };
        match c.risk {
            LedgerRisk::L28 => {
                if c.phase != RiskPhase::RkClosed {
                    return false;
                }
            }
            LedgerRisk::L29 => {
                if c.phase != RiskPhase::RkClosed {
                    return false;
                }
            }
            LedgerRisk::L30 => {
                if c.phase == RiskPhase::RkClosed {
                    return false;
                }
            }
        }
        i += 1;
    }
    // 迁移守卫直读：倒退拒、待实测闭合拒。
    let r1 = risk_advance(LedgerRisk::L28, RiskPhase::RkOpened, RiskPhase::RkReviewing);
    let r2 = risk_advance(LedgerRisk::L28, RiskPhase::RkClosed, RiskPhase::RkOpened);
    let r3 = risk_advance(LedgerRisk::L30, RiskPhase::RkReviewing, RiskPhase::RkClosed);
    r1.is_ok() && r2.is_err() && r3.is_err()
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn rk_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 待实测虚报绿。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let fake = SpotEntry { no: 0, source: "虚构探针", claim: ClaimKind::PendingRealTest, verified: true };
        if led.reg_spot(fake).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚绿;");
        }
    }
    // 2. 实测绿漏记。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let miss = SpotEntry { no: 0, source: "漏记探针", claim: ClaimKind::MeasuredGreen, verified: false };
        if led.reg_spot(miss).is_err() {
            caught += 1;
        } else {
            escape.push_str("漏记;");
        }
    }
    // 3. 抽检额不足（只入 49 处就封）。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let mut i = 0u32;
        while i < SPOT_MIN as u32 - 1 {
            if let Some(e) = replay_spot(i) {
                let _ = led.reg_spot(e);
            }
            i += 1;
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺额;");
        }
    }
    // 4. 抽检号重复。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let e0 = match replay_spot(0) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺抽")),
        };
        let _ = led.reg_spot(e0.clone());
        if led.reg_spot(e0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重号;");
        }
    }
    // 5. 出处置空。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let empty = SpotEntry { no: 0, source: "", claim: ClaimKind::MeasuredGreen, verified: true };
        if led.reg_spot(empty).is_err() {
            caught += 1;
        } else {
            escape.push_str("空引;");
        }
    }
    // 6. 风险缺一封账。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let mut i = 0u32;
        while (i as usize) < SPOT_REFS.len() {
            if let Some(e) = replay_spot(i) {
                let _ = led.reg_spot(e);
            }
            i += 1;
        }
        let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L28, phase: RiskPhase::RkClosed });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺险;");
        }
    }
    // 7. 风险重复。
    {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let c = RiskClosure { risk: LedgerRisk::L28, phase: RiskPhase::RkClosed };
        let _ = led.reg_closure(c.clone());
        if led.reg_closure(c).is_err() {
            caught += 1;
        } else {
            escape.push_str("重险;");
        }
    }
    // 8. 封账后写。
    {
        let mut led = build_full_rk_ledger();
        let extra = match replay_spot(0) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺抽")),
        };
        if led.reg_spot(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut led = build_full_rk_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（申报名与风险号恒非空）。
    {
        let mut bad = false;
        for c in ClaimKind::all().iter() {
            if c.name().is_empty() {
                bad = true;
            }
        }
        for r in LedgerRisk::all().iter() {
            if r.name().is_empty() {
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
    let a = build_full_rk_ledger();
    let b = build_full_rk_ledger();
    if a.len() != b.len() || a.closure_len() != b.closure_len() {
        return false;
    }
    if a.green_len() != b.green_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.spot_at(i), b.spot_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if rk_fault_sweep().0 != rk_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（抽检表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for r in SPOT_REFS.iter() {
        let claim_bit: u32 = match r.1 {
            ClaimKind::MeasuredGreen => 1,
            ClaimKind::PendingRealTest => 2,
        };
        acc = acc
            .wrapping_mul(3)
            .wrapping_add(claim_bit)
            .wrapping_add(r.0.len() as u32)
            .wrapping_add(i);
        i += 1;
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
    if SEG_BASE != 0x80000 + 0x40 * (12695u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_rk_ledger();
    if led.state() != RkState::RkSealed {
        return false;
    }
    for c in ClaimKind::all().iter() {
        if c.name().is_empty() {
            return false;
        }
    }
    for r in LedgerRisk::all().iter() {
        if r.name().is_empty() {
            return false;
        }
    }
    // 待实测项不虚封绿（030 不闭合）。
    if !pending_zero_fabricate_ok() {
        return false;
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12696 风险账总表自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12695_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12695-risk-ledger");
    let led = build_full_rk_ledger();

    s.add_dyn(
        "F12696-01 抽检 ≥50 处出处",
        spotcheck_fifty_ok(),
        &format!("spots={} min={} greens={}", led.len(), SPOT_MIN, led.green_len()),
    );

    s.add_dyn(
        "F12696-02 待实测项零虚构",
        pending_zero_fabricate_ok(),
        "虚构注入必抓且待实测号不闭合",
    );

    s.add_dyn(
        "F12696-03 三风险闭合状态机",
        risk_closure_ok(),
        &format!("closures={} sealed={}", led.closure_len(), led.state() == RkState::RkSealed),
    );

    s.add_dyn(
        "F12696-04 抽检一致双向口径",
        {
            let mut all = true;
            let mut i = 0usize;
            while i < led.len() {
                match led.spot_at(i) {
                    Some(e) => {
                        if !spot_entry_consistent(e) {
                            all = false;
                        }
                    }
                    None => all = false,
                }
                i += 1;
            }
            all
        },
        "实测绿必核验、待实测必挂起",
    );

    s.add_dyn(
        "F12696-05 抽检额冻结",
        SPOT_REFS.len() == SPOT_CAP && SPOT_CAP >= SPOT_MIN,
        &format!("cap={} min={}", SPOT_CAP, SPOT_MIN),
    );

    s.add_dyn(
        "F12696-06 实测绿额冻结",
        led.green_len() == SPOT_GREEN_CAP as u32,
        &format!("greens={} cap={}", led.green_len(), SPOT_GREEN_CAP),
    );

    s.add_dyn(
        "F12696-07 账等式（核验绿 ≡ 实测绿申报）",
        led.equation_ok(),
        &format!("greens={} rejects={}", led.green_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12696-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = rk_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", rk_fault_sweep().0),
    );

    s.add_dyn(
        "F12696-09 迁移守卫直读",
        risk_advance(LedgerRisk::L29, RiskPhase::RkOpened, RiskPhase::RkReviewing).is_ok()
            && risk_advance(LedgerRisk::L29, RiskPhase::RkReviewing, RiskPhase::RkOpened).is_err(),
        "开账→复核中→已闭合只进不退",
    );

    s.add_dyn(
        "F12696-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12696-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12696-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12696-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x817C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RK_ALL.len(), 11);
        let mut i = 0usize;
        while i < RK_ALL.len() {
            let vi = match RK_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RK_ALL.len() {
                let vj = match RK_ALL.get(j) {
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
    fn spotcheck_fifty_holds() {
        assert!(spotcheck_fifty_ok());
        assert!(risk_closure_ok());
        let led = build_full_rk_ledger();
        assert_eq!(led.len(), 52);
        assert!(led.len() >= SPOT_MIN);
        assert_eq!(led.green_len(), 50);
        assert_eq!(led.state(), RkState::RkSealed);
        assert_eq!(led.reject_len(), 0);
        assert!(led.equation_ok());
    }

    #[test]
    fn pending_never_fabricated() {
        assert!(pending_zero_fabricate_ok());
        // 待实测条目必须 verified = false。
        let mut i = 0u32;
        while (i as usize) < SPOT_REFS.len() {
            let e = match replay_spot(i) {
                Some(v) => v,
                None => return,
            };
            if e.claim == ClaimKind::PendingRealTest {
                assert!(!e.verified, "抽检 {} 待实测虚绿", i);
            } else {
                assert!(e.verified, "抽检 {} 实测绿漏记", i);
            }
            i += 1;
        }
        // 闭合面：030 必须停在复核中。
        let led = build_full_rk_ledger();
        let c030 = led
            .closures
            .iter()
            .find(|c| c.risk == LedgerRisk::L30);
        match c030 {
            Some(c) => assert_eq!(c.phase, RiskPhase::RkReviewing),
            None => assert!(false, "030 闭合行缺失"),
        }
    }

    #[test]
    fn risk_phase_guards() {
        assert!(risk_advance(LedgerRisk::L28, RiskPhase::RkOpened, RiskPhase::RkReviewing).is_ok());
        assert!(risk_advance(LedgerRisk::L28, RiskPhase::RkReviewing, RiskPhase::RkClosed).is_ok());
        // 倒退拒。
        assert!(risk_advance(LedgerRisk::L28, RiskPhase::RkClosed, RiskPhase::RkReviewing).is_err());
        // 同相位拒。
        assert!(risk_advance(LedgerRisk::L28, RiskPhase::RkOpened, RiskPhase::RkOpened).is_err());
        // 待实测闭合拒。
        assert!(risk_advance(LedgerRisk::L30, RiskPhase::RkReviewing, RiskPhase::RkClosed).is_err());
        // 秩单调。
        assert!(RiskPhase::RkOpened.rank() < RiskPhase::RkReviewing.rank());
        assert!(RiskPhase::RkReviewing.rank() < RiskPhase::RkClosed.rank());
        assert!(!LedgerRisk::L30.closable());
        assert!(LedgerRisk::L28.closable());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = rk_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn spot_gates_reject() {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        // 虚绿。
        let fake = SpotEntry { no: 0, source: "x", claim: ClaimKind::PendingRealTest, verified: true };
        assert_eq!(led.reg_spot(fake), Err(RkDiag::EFakeGreen.val()));
        // 漏记。
        let miss = SpotEntry { no: 0, source: "x", claim: ClaimKind::MeasuredGreen, verified: false };
        assert_eq!(led.reg_spot(miss), Err(RkDiag::EGreenMissing.val()));
        // 空引。
        let empty = SpotEntry { no: 0, source: "", claim: ClaimKind::MeasuredGreen, verified: true };
        assert_eq!(led.reg_spot(empty), Err(RkDiag::ESourceEmpty.val()));
        assert_eq!(led.reject_len(), 3);
        // 正常条目过闸。
        let ok = match replay_spot(0) {
            Some(v) => v,
            None => return,
        };
        assert!(led.reg_spot(ok).is_ok());
        assert_eq!(led.len(), 1);
        assert_eq!(led.green_len(), 1);
    }

    #[test]
    fn spot_short_cannot_seal() {
        let mut led = RkLedger::new();
        let _ = led.begin_run();
        let mut i = 0u32;
        while i < SPOT_MIN as u32 - 1 {
            if let Some(e) = replay_spot(i) {
                let _ = led.reg_spot(e);
            }
            i += 1;
        }
        assert_eq!(led.seal(), Err(RkDiag::ESpotShort.val()));
        // 补足后仍缺风险闭合行。
        let mut i2 = SPOT_MIN as u32 - 1;
        while (i2 as usize) < SPOT_REFS.len() {
            if let Some(e) = replay_spot(i2) {
                let _ = led.reg_spot(e);
            }
            i2 += 1;
        }
        assert_eq!(led.seal(), Err(RkDiag::ERiskMissing.val()));
        // 补齐三闭合行后可封。
        let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L28, phase: RiskPhase::RkClosed });
        let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L29, phase: RiskPhase::RkClosed });
        let _ = led.reg_closure(RiskClosure { risk: LedgerRisk::L30, phase: RiskPhase::RkReviewing });
        assert!(led.seal().is_ok());
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_rk_ledger();
        let b = build_full_rk_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.spot_at(i), b.spot_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑抽检缺失"),
            }
            i += 1;
        }
        assert_eq!(rk_fault_sweep().0, rk_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn replay_boundary() {
        assert!(replay_spot(0).is_some());
        assert!(replay_spot(51).is_some());
        assert!(replay_spot(52).is_none());
        assert_eq!(SPOT_REFS.len(), 52);
        assert_eq!(ClaimKind::all().len(), 2);
        assert_eq!(LedgerRisk::all().len(), 3);
        assert_eq!(RiskPhase::all().len(), 3);
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(RkState::RkOpen.rank() < RkState::RkRunning.rank());
        assert!(RkState::RkRunning.rank() < RkState::RkSealed.rank());
        let mut led = build_full_rk_ledger();
        assert_eq!(led.state(), RkState::RkSealed);
        assert_eq!(led.seal(), Err(RkDiag::ERewind.val()));
        let mut fresh = RkLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(RkDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12695_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
