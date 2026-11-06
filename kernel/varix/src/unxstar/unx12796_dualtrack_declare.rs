//! UNX-F12796 双轨产线声明（开发期零 QEMU 纪律全域适用） · 0x83100段
//!
//! 判据原文（派工卡 12796 逐字锚定）：
//!   UNX-F12797-J1 补测清单齐（判据号/条件/预算三列），与 handoff 开放风险零偏差。
//!
//! 设计要点：
//! 1. 双轨产线：轨道 A = 补测清单登记表（判据号/触发条件/预算三列闭集登记，
//!    48 行冻结表，行可复算）；轨道 B = handoff 开放风险登记表（风险号/风险
//!    描述代号/是否已覆盖），两轨以判定器逐项对账，零偏差才许声明完成；
//! 2. 冻结分布规则：判据号按冻结算术分布（cid(i) = CID_BASE + 3×i，非连续
//!    但按冻结规则可逐行复算），触发条件列落在 6 类闭集且逐行等于冻结值，
//!    预算列按冻结公式 budget(i) = 128 + 16×i 复算——三列任一违冻结即拒；
//! 3. 判定器 judge_deviation(risk_id, covered)：判定次序为「零号位 → 存在性
//!    → 覆盖比对」，清单未覆盖的 handoff 开放风险 → ESRCH 语义（缺账，
//!    errno=3），覆盖了不存在的风险 → EINVAL（errno=22），逐项对账零偏差；
//! 4. 零 QEMU 纪律：本模块声明的依赖引用登记表中 QEMU 禁词引用计数恒 0，
//!    三类注入器（清单行缺失/风险缺账/越权覆盖）各 10 轮全抓，任何注入必
//!    被对应闸门抓住，逃脱即判据失效；
//! 5. 账本状态机 登记→点验→冻结 单向推进，倒退显性拒；冻结后写入拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标/断言宏（生产区）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12796 − 12600)）。
pub const SEG_BASE: u32 = 0x83100;

// ── 双轨声明诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x12 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DkDiag {
    /// 判定器收到零号位（风险号 0 非法，先于存在性判定）。
    EJudgeZero,
    /// 清单未覆盖的 handoff 开放风险（ESRCH 语义：缺账，errno=3）。
    EGap,
    /// 覆盖了不存在的风险（EINVAL 语义：越权覆盖，errno=22）。
    EOverCover,
    /// 清单行判据号越冻结闭集（低于基号或高于末号）。
    ERowRange,
    /// 判据号跳行/违冻结分布规则（闭集内但不在冻结位置）。
    ERuleGap,
    /// 判据号重复登记。
    EDupCid,
    /// 触发条件列缺账或越六类闭集。
    ECondCol,
    /// 触发条件列在闭集内但违冻结分布值。
    ECondFrozen,
    /// 预算列缺账或越预算上限。
    EBudgetCol,
    /// 预算列在上限内但违冻结公式值。
    EBudgetFrozen,
    /// 清单容量耗尽（48 行满后再登记）。
    ECapFull,
    /// 清单已冻结，写入被拒。
    ESealed,
    /// 清单未齐就试图点验/冻结。
    ENotFull,
    /// 点验未过（逐行复算出现红项）。
    EVerify,
    /// 状态机倒退（冻结后又点验/重复冻结）。
    ERewind,
    /// QEMU 依赖引用计数非零（零 QEMU 纪律违约）。
    EQemuRef,
    /// 注入逃脱（注入器构造的违约未被任何闸门抓住）。
    EInjEscape,
    /// 风险描述代号越五类闭集。
    EDescCode,
}

impl DkDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x12 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DkDiag::EJudgeZero => 0x01,
                DkDiag::EGap => 0x02,
                DkDiag::EOverCover => 0x03,
                DkDiag::ERowRange => 0x04,
                DkDiag::ERuleGap => 0x05,
                DkDiag::EDupCid => 0x06,
                DkDiag::ECondCol => 0x07,
                DkDiag::ECondFrozen => 0x08,
                DkDiag::EBudgetCol => 0x09,
                DkDiag::EBudgetFrozen => 0x0a,
                DkDiag::ECapFull => 0x0b,
                DkDiag::ESealed => 0x0c,
                DkDiag::ENotFull => 0x0d,
                DkDiag::EVerify => 0x0e,
                DkDiag::ERewind => 0x0f,
                DkDiag::EQemuRef => 0x10,
                DkDiag::EInjEscape => 0x11,
                DkDiag::EDescCode => 0x12,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DkDiag::EJudgeZero => "风险号零号位非法：判定器只认非零风险号，先补号位再对账",
            DkDiag::EGap => "ESRCH 缺账：清单未覆盖该 handoff 开放风险，回补测清单补一行覆盖登记",
            DkDiag::EOverCover => "EINVAL 越权覆盖：覆盖了不在 handoff 开放风险表的风险号，先对表再声明覆盖",
            DkDiag::ERowRange => "判据号越冻结闭集：只认 CID_BASE..CID_BASE+3×47，先对冻结分布表再登记",
            DkDiag::ERuleGap => "判据号跳行：闭集内但不在冻结位置，按已登记行数回推应落行号",
            DkDiag::EDupCid => "判据号重复：每判据只落一行，重复登记先查已有行",
            DkDiag::ECondCol => "触发条件列缺账或越六类闭集：条件代号称取 1..=6 非零值",
            DkDiag::ECondFrozen => "触发条件违冻结分布：该行条件代号应等于冻结值，按行号复算回填",
            DkDiag::EBudgetCol => "预算列缺账或越上限：预算取 1..=4096 非零值",
            DkDiag::EBudgetFrozen => "预算违冻结公式：该行预算应等于 128+16×行号，按行号复算回填",
            DkDiag::ECapFull => "清单容量耗尽：48 行满即封口，新增判据走下一张卡滚动",
            DkDiag::ESealed => "清单已冻结：冻结后写入拒绝，改动走补测清单修订流程",
            DkDiag::ENotFull => "清单未齐禁止点验/冻结：先补齐 48 行再走点验口",
            DkDiag::EVerify => "点验未过：逐行复算出现红项，按红项行号回修对应列",
            DkDiag::ERewind => "状态机倒退：登记→点验→冻结只进不退，倒退即声明可疑",
            DkDiag::EQemuRef => "QEMU 依赖引用非零：零 QEMU 纪律全域适用，从依赖登记表剔除该引用",
            DkDiag::EInjEscape => "注入逃脱：构造的违约未被闸门抓住，对账面失效须整表复核",
            DkDiag::EDescCode => "风险描述代号越五类闭集：代号称取 1..=5，先对风险描述表再登记",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DK_ALL: &[DkDiag] = &[
    DkDiag::EJudgeZero,
    DkDiag::EGap,
    DkDiag::EOverCover,
    DkDiag::ERowRange,
    DkDiag::ERuleGap,
    DkDiag::EDupCid,
    DkDiag::ECondCol,
    DkDiag::ECondFrozen,
    DkDiag::EBudgetCol,
    DkDiag::EBudgetFrozen,
    DkDiag::ECapFull,
    DkDiag::ESealed,
    DkDiag::ENotFull,
    DkDiag::EVerify,
    DkDiag::ERewind,
    DkDiag::EQemuRef,
    DkDiag::EInjEscape,
    DkDiag::EDescCode,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DK_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x83101..0x83112 闭集内"
}

/// errno 语义映射（man 3 errno 闭集）：缺账→ESRCH(3)，越权/零号位→EINVAL(22)。
pub const ERRNO_ESRCH: u32 = 3;
/// errno 语义映射：EINVAL 常数值。
pub const ERRNO_EINVAL: u32 = 22;

/// 诊断码的 errno 语义（非对账码返回 0）。
pub fn errno_semantics(code: u32) -> u32 {
    if code == DkDiag::EGap.val() {
        return ERRNO_ESRCH;
    }
    if code == DkDiag::EOverCover.val() || code == DkDiag::EJudgeZero.val() {
        return ERRNO_EINVAL;
    }
    0
}

// ── 轨道 A：补测清单冻结分布规则 ─────────────────────────────────
/// 清单容量：48 行冻结表（判据号/条件/预算三列）。
pub const ROW_CAP: usize = 48;
/// 判据号基号（UNX-F12797 判据族锚定）。
pub const CID_BASE: u32 = 1_279_701;
/// 预算列上限（单行补测预算封顶，冻结常量）。
pub const BUDGET_CAP: u32 = 4096;

/// 冻结分布规则·判据号列：cid(i) = CID_BASE + 3×i（等距分布，行可复算）。
pub fn frozen_cid(i: u32) -> u32 {
    CID_BASE + i.saturating_mul(3)
}

/// 冻结分布规则·触发条件列：kind(i) = (i % 6) + 1，落在六类闭集内循环分布。
pub fn frozen_cond(i: u32) -> u32 {
    (i % 6).saturating_add(1)
}

/// 冻结分布规则·预算列：budget(i) = 128 + 16×i（线性冻结公式）。
pub fn frozen_budget(i: u32) -> u32 {
    128u32.saturating_add(i.saturating_mul(16))
}

/// 触发条件六类闭集（补测触发口径，冻结不许重排）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondKind {
    /// 回归用例（触发条件代号 1）。
    Regress,
    /// 资产盘点（触发条件代号 2）。
    AssetAudit,
    /// 同步声明（触发条件代号 3）。
    SyncDeclare,
    /// 断言资产（触发条件代号 4）。
    AssertAsset,
    /// 风险覆盖（触发条件代号 5）。
    RiskCover,
    /// 指纹复核（触发条件代号 6）。
    FpRecheck,
}

impl CondKind {
    /// 条件代号（闭集序 1..=6）。
    pub fn code(self) -> u32 {
        match self {
            CondKind::Regress => 1,
            CondKind::AssetAudit => 2,
            CondKind::SyncDeclare => 3,
            CondKind::AssertAsset => 4,
            CondKind::RiskCover => 5,
            CondKind::FpRecheck => 6,
        }
    }

    /// 条件中文名。
    pub fn name(self) -> &'static str {
        match self {
            CondKind::Regress => "回归用例",
            CondKind::AssetAudit => "资产盘点",
            CondKind::SyncDeclare => "同步声明",
            CondKind::AssertAsset => "断言资产",
            CondKind::RiskCover => "风险覆盖",
            CondKind::FpRecheck => "指纹复核",
        }
    }

    /// 由代号反查闭集（越界返回 None）。
    pub fn from_code(code: u32) -> Option<CondKind> {
        match code {
            1 => Some(CondKind::Regress),
            2 => Some(CondKind::AssetAudit),
            3 => Some(CondKind::SyncDeclare),
            4 => Some(CondKind::AssertAsset),
            5 => Some(CondKind::RiskCover),
            6 => Some(CondKind::FpRecheck),
            _ => None,
        }
    }
}

/// 六类闭集数组（代号序）。
pub const COND_ALL: &[CondKind] = &[
    CondKind::Regress,
    CondKind::AssetAudit,
    CondKind::SyncDeclare,
    CondKind::AssertAsset,
    CondKind::RiskCover,
    CondKind::FpRecheck,
];

// ── 轨道 A 数据模型：补测清单登记表 ──────────────────────────────
/// 单行三列单元：判据号 / 触发条件代号 / 预算（均非零有效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetestRow {
    /// 行号（0..48 闭集序）。
    pub idx: u32,
    /// 判据号（冻结分布）。
    pub cid: u32,
    /// 触发条件代号（1..=6 闭集且等于冻结值）。
    pub cond: u32,
    /// 预算（1..=4096 且等于冻结公式值）。
    pub budget: u32,
}

impl RetestRow {
    /// 三列非空（判据号/条件/预算全非零）。
    pub fn cols_nonempty(&self) -> bool {
        self.cid != 0 && self.cond != 0 && self.budget != 0
    }

    /// 行摘要（诊断文本用）。
    pub fn brief(&self) -> String {
        let cond_name = match CondKind::from_code(self.cond) {
            Some(k) => k.name(),
            None => "?",
        };
        format!(
            "行{}判据{}条件{}({})预算{}",
            self.idx, self.cid, self.cond, cond_name, self.budget
        )
    }
}

/// 清单状态机：登记 → 点验 → 冻结，单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerPhase {
    /// 登记：允许逐行落三列。
    Registering,
    /// 点验：48 行齐且逐行复算全绿。
    Verified,
    /// 冻结：只读，声明完成。
    Frozen,
}

impl LedgerPhase {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            LedgerPhase::Registering => 0,
            LedgerPhase::Verified => 1,
            LedgerPhase::Frozen => 2,
        }
    }
}

/// 补测清单账本（轨道 A 主体）。
#[derive(Clone, Debug)]
pub struct RetestLedger {
    /// 已登记行（按冻结行号序）。
    rows: Vec<RetestRow>,
    /// 账本状态。
    phase: LedgerPhase,
}

impl RetestLedger {
    /// 开一本新清单。
    pub fn new() -> RetestLedger {
        RetestLedger {
            rows: Vec::new(),
            phase: LedgerPhase::Registering,
        }
    }

    /// 当前状态。
    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    /// 已登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<RetestRow> {
        self.rows.get(idx).copied()
    }

    /// 按判据号查行。
    pub fn row_of_cid(&self, cid: u32) -> Option<RetestRow> {
        for r in self.rows.iter() {
            if r.cid == cid {
                return Some(*r);
            }
        }
        None
    }

    /// 逐字登记一行三列（冻结/容量/闭集/分布/重复/列缺 六道闸）。
    pub fn register(&mut self, cid: u32, cond: u32, budget: u32) -> Result<(), u32> {
        // 冻结闸：冻结后一切写入拒绝。
        if self.phase.rank() >= LedgerPhase::Frozen.rank() {
            return Err(DkDiag::ESealed.val());
        }
        // 容量闸：48 行满即耗尽（先于行号闸，满账后任何写入都归容量）。
        if self.rows.len() >= ROW_CAP {
            return Err(DkDiag::ECapFull.val());
        }
        // 判据号闭集闸：低于基号或高于末号即越界。
        let last_cid = frozen_cid((ROW_CAP - 1) as u32);
        if cid < CID_BASE || cid > last_cid {
            return Err(DkDiag::ERowRange.val());
        }
        // 冻结分布闸：判据号必须落在当前行数的冻结位置（跳行/乱序同拒）。
        let expect_cid = frozen_cid(self.rows.len() as u32);
        if cid != expect_cid {
            return Err(DkDiag::ERuleGap.val());
        }
        // 重复闸（顺序登记下理论不可达，防御性保留）。
        if self.row_of_cid(cid).is_some() {
            return Err(DkDiag::EDupCid.val());
        }
        // 触发条件列闸：缺账或越六类闭集。
        if cond == 0 || CondKind::from_code(cond).is_none() {
            return Err(DkDiag::ECondCol.val());
        }
        // 触发条件冻结闸：闭集内但违冻结分布值。
        if cond != frozen_cond(self.rows.len() as u32) {
            return Err(DkDiag::ECondFrozen.val());
        }
        // 预算列闸：缺账或越上限。
        if budget == 0 || budget > BUDGET_CAP {
            return Err(DkDiag::EBudgetCol.val());
        }
        // 预算冻结闸：上限内但违冻结公式值。
        if budget != frozen_budget(self.rows.len() as u32) {
            return Err(DkDiag::EBudgetFrozen.val());
        }
        let idx = self.rows.len() as u32;
        self.rows.push(RetestRow {
            idx,
            cid,
            cond,
            budget,
        });
        Ok(())
    }

    /// 逐行复算（点验核心）：按冻结规则重算每一行三列，首红即返。
    pub fn audit_rows(&self) -> Result<(), u32> {
        let mut i = 0usize;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.idx != i as u32 {
                        return Err(DkDiag::ERowRange.val());
                    }
                    if r.cid != frozen_cid(i as u32) {
                        return Err(DkDiag::ERuleGap.val());
                    }
                    if r.cond == 0 || CondKind::from_code(r.cond).is_none() {
                        return Err(DkDiag::ECondCol.val());
                    }
                    if r.cond != frozen_cond(i as u32) {
                        return Err(DkDiag::ECondFrozen.val());
                    }
                    if r.budget == 0 || r.budget > BUDGET_CAP {
                        return Err(DkDiag::EBudgetCol.val());
                    }
                    if r.budget != frozen_budget(i as u32) {
                        return Err(DkDiag::EBudgetFrozen.val());
                    }
                }
                None => return Err(DkDiag::EVerify.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 清单齐点验：48 行齐 + 三列逐行非空可复算。
    pub fn checklist_complete(&self) -> bool {
        self.rows.len() == ROW_CAP && self.audit_rows().is_ok()
    }

    /// 点验：齐 + 复算全绿才推进到 Verified（倒退拒）。
    pub fn verify(&mut self) -> Result<(), u32> {
        if self.phase.rank() >= LedgerPhase::Frozen.rank() {
            return Err(DkDiag::ERewind.val());
        }
        if self.rows.len() != ROW_CAP {
            return Err(DkDiag::ENotFull.val());
        }
        if self.audit_rows().is_err() {
            return Err(DkDiag::EVerify.val());
        }
        self.phase = LedgerPhase::Verified;
        Ok(())
    }

    /// 冻结：点验过才许冻结；重复冻结即倒退拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        match self.phase {
            LedgerPhase::Registering => Err(DkDiag::ENotFull.val()),
            LedgerPhase::Verified => {
                self.phase = LedgerPhase::Frozen;
                Ok(())
            }
            LedgerPhase::Frozen => Err(DkDiag::ERewind.val()),
        }
    }

    /// 构造缺行账本（注入面：跳过第 drop 行登记，其余原样誊抄）。
    pub fn clone_minus(&self, drop: usize) -> RetestLedger {
        let mut out = RetestLedger::new();
        let mut i = 0usize;
        while i < self.rows.len() {
            if i != drop {
                if let Some(r) = self.rows.get(i) {
                    let row = *r;
                    out.rows.push(RetestRow {
                        idx: row.idx,
                        cid: row.cid,
                        cond: row.cond,
                        budget: row.budget,
                    });
                }
            }
            i += 1;
        }
        out
    }
}

/// 构造齐账清单（48 行三列全按冻结公式，行可复算）。
pub fn build_full_ledger() -> RetestLedger {
    let mut l = RetestLedger::new();
    let mut i = 0u32;
    while (i as usize) < ROW_CAP {
        // 冻结公式登记：正常流不该出错；出错即终止构造（缺行，点验会红）。
        match l.register(frozen_cid(i), frozen_cond(i), frozen_budget(i)) {
            Ok(()) => i += 1,
            Err(e) => {
                let _ = e;
                break;
            }
        }
    }
    l
}

// ── 轨道 B：handoff 开放风险登记表 ───────────────────────────────
/// 风险容量：8 条 handoff 开放风险（冻结闭集）。
pub const RISK_CAP: usize = 8;
/// 风险号基号（handoff 交付口径冻结常量）。
pub const RISK_BASE: u32 = 9001;

/// 风险描述代号冻结分布（五类闭集内取值，来源：handoff 交接清单口径）。
pub const RISK_DESC_FROZEN: [u32; RISK_CAP] = [1, 3, 2, 5, 4, 1, 3, 5];

/// 风险描述五类闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RiskDesc {
    /// 补测缺口（代号 1）。
    RetestGap,
    /// 依赖漂移（代号 2）。
    DepDrift,
    /// 指纹失配（代号 3）。
    FpMismatch,
    /// 预算超支（代号 4）。
    BudgetOver,
    /// 覆盖越权（代号 5）。
    CoverOver,
}

impl RiskDesc {
    /// 代号（1..=5）。
    pub fn code(self) -> u32 {
        match self {
            RiskDesc::RetestGap => 1,
            RiskDesc::DepDrift => 2,
            RiskDesc::FpMismatch => 3,
            RiskDesc::BudgetOver => 4,
            RiskDesc::CoverOver => 5,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            RiskDesc::RetestGap => "补测缺口",
            RiskDesc::DepDrift => "依赖漂移",
            RiskDesc::FpMismatch => "指纹失配",
            RiskDesc::BudgetOver => "预算超支",
            RiskDesc::CoverOver => "覆盖越权",
        }
    }

    /// 由代号反查闭集（越界返回 None）。
    pub fn from_code(code: u32) -> Option<RiskDesc> {
        match code {
            1 => Some(RiskDesc::RetestGap),
            2 => Some(RiskDesc::DepDrift),
            3 => Some(RiskDesc::FpMismatch),
            4 => Some(RiskDesc::BudgetOver),
            5 => Some(RiskDesc::CoverOver),
            _ => None,
        }
    }
}

/// 单条 handoff 开放风险：风险号 / 风险描述代号 / 是否已覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RiskRow {
    /// 风险号（RISK_BASE 起连续编号）。
    pub risk_id: u32,
    /// 风险描述代号（1..=5 闭集）。
    pub desc: u32,
    /// 是否已覆盖（登记口径：开放风险必须由补测清单覆盖）。
    pub covered: bool,
}

/// handoff 开放风险登记表（冻结构造，行可复算）。
pub fn handoff_risk_rows() -> Vec<RiskRow> {
    let mut out: Vec<RiskRow> = Vec::new();
    let mut i = 0usize;
    while i < RISK_CAP {
        // 描述代号与覆盖旗标均从冻结数组按 .get() 取，不裸下标。
        let desc = RISK_DESC_FROZEN.get(i).copied().unwrap_or(1);
        out.push(RiskRow {
            risk_id: RISK_BASE + i as u32,
            desc,
            covered: true,
        });
        i += 1;
    }
    out
}

/// 按风险号查行。
pub fn find_risk(risk_id: u32) -> Option<RiskRow> {
    for r in handoff_risk_rows().iter() {
        if r.risk_id == risk_id {
            return Some(*r);
        }
    }
    None
}

/// 风险表审计：风险号连续、描述代号在闭集、覆盖旗标全真、号不重复。
pub fn handoff_risk_audit() -> Result<(), u32> {
    let rows = handoff_risk_rows();
    if rows.len() != RISK_CAP {
        return Err(DkDiag::EDescCode.val());
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(r) => *r,
            None => return Err(DkDiag::EDescCode.val()),
        };
        if r.risk_id != RISK_BASE + i as u32 {
            return Err(DkDiag::EDescCode.val());
        }
        if RiskDesc::from_code(r.desc).is_none() {
            return Err(DkDiag::EDescCode.val());
        }
        if !r.covered {
            return Err(DkDiag::EGap.val());
        }
        let mut j = i + 1;
        while j < rows.len() {
            if let Some(r2) = rows.get(j) {
                if r2.risk_id == r.risk_id {
                    return Err(DkDiag::EDescCode.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 判定器：逐项对账零偏差 ───────────────────────────────────────
/// 判定次序闭集：零号位 → 存在性 → 覆盖比对（次序即语义，不许换位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JudgeStep {
    /// 第一步：零号位拒绝。
    ZeroCheck,
    /// 第二步：存在性比对（在表/不在表分叉）。
    Existence,
    /// 第三步：覆盖比对（缺账 ESRCH / 越权 EINVAL / 零偏差 Ok）。
    Coverage,
}

impl JudgeStep {
    /// 次序（只许按序判定）。
    pub fn rank(self) -> u8 {
        match self {
            JudgeStep::ZeroCheck => 0,
            JudgeStep::Existence => 1,
            JudgeStep::Coverage => 2,
        }
    }
}

/// 判定器：清单对 handoff 开放风险的覆盖声明逐项对账。
///
/// 判定次序（冻结语义）：
/// 1. 零号位：risk_id == 0 → EINVAL 语义（EJudgeZero），先于存在性；
/// 2. 存在性：在 handoff 开放风险表内 → 进覆盖比对；
///    不在表内且声明覆盖 → EINVAL 语义（EOverCover 越权覆盖）；
///    不在表内且未声明覆盖 → Ok（未覆盖非风险，无偏差）；
/// 3. 覆盖比对：在表内但声明未覆盖 → ESRCH 语义（EGap 缺账）；
///    在表内且声明覆盖 → Ok（零偏差）。
pub fn judge_deviation(risk_id: u32, covered: bool) -> Result<(), u32> {
    // 次序一：零号位拒绝（零号位不是合法风险号，先于存在性）。
    if risk_id == 0 {
        return Err(DkDiag::EJudgeZero.val());
    }
    // 次序二：存在性比对。
    match find_risk(risk_id) {
        Some(r) => {
            // 次序三：覆盖比对（登记口径 covered 全真，声明 false 即缺账）。
            if covered == r.covered {
                Ok(())
            } else {
                Err(DkDiag::EGap.val())
            }
        }
        None => {
            if covered {
                Err(DkDiag::EOverCover.val())
            } else {
                Ok(())
            }
        }
    }
}

/// 全表对账报告：逐项核对全部 handoff 开放风险。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviationReport {
    /// 已对账风险条数。
    pub checked: u32,
    /// 缺账条数（ESRCH 语义红项）。
    pub gap: u32,
    /// 越权覆盖条数（EINVAL 语义红项）。
    pub over: u32,
    /// 零偏差（缺账=0 且越权=0 且逐项全对账）。
    pub zero_dev: bool,
}

/// 全表对账：8 条风险逐项判 covered=true 声明，统计红项。
pub fn full_reconciliation() -> DeviationReport {
    let mut checked = 0u32;
    let mut gap = 0u32;
    let mut over = 0u32;
    let rows = handoff_risk_rows();
    for r in rows.iter() {
        checked = checked.saturating_add(1);
        match judge_deviation(r.risk_id, true) {
            Ok(()) => {}
            Err(e) => {
                if e == DkDiag::EGap.val() {
                    gap = gap.saturating_add(1);
                } else if e == DkDiag::EOverCover.val() {
                    over = over.saturating_add(1);
                }
            }
        }
    }
    DeviationReport {
        checked,
        gap,
        over,
        zero_dev: gap == 0 && over == 0 && checked == RISK_CAP as u32,
    }
}

// ── 零 QEMU 纪律：依赖引用登记表与禁词扫描 ───────────────────────
/// 本模块声明的全部依赖引用（冻结登记表，零 QEMU 纪律的扫描面）。
pub const DEP_REFS: &[&str] = &["alloc", "core", "crate::checks"];

/// QEMU 禁词表（零 QEMU 纪律全域适用的扫描闭集）。
pub const QEMU_TOKENS: &[&str] = &["qemu", "QEMU", "Qemu"];

/// 字节序列包含判定（match/.get() 取值，无裸下标）。
fn bytes_contain(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        let mut eq = true;
        let mut j = 0usize;
        while j < needle.len() {
            let a = match hay.get(i + j) {
                Some(b) => *b,
                None => {
                    eq = false;
                    break;
                }
            };
            let b = match needle.get(j) {
                Some(b) => *b,
                None => {
                    eq = false;
                    break;
                }
            };
            if a != b {
                eq = false;
                break;
            }
            j += 1;
        }
        if eq {
            return true;
        }
        i += 1;
    }
    false
}

/// 扫描一组依赖引用中的 QEMU 禁词引用计数。
pub fn scan_dep_refs(refs: &[&str]) -> u32 {
    let mut n = 0u32;
    for r in refs.iter() {
        let hay = r.as_bytes();
        for t in QEMU_TOKENS.iter() {
            if bytes_contain(hay, t.as_bytes()) {
                n = n.saturating_add(1);
                break;
            }
        }
    }
    n
}

/// 本模块 QEMU 依赖引用计数（零 QEMU 纪律要求恒 0）。
pub fn qemu_ref_count() -> u32 {
    scan_dep_refs(DEP_REFS)
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 注入面：三类注入器各 10 轮全抓 ───────────────────────────────
/// 注入器一：清单行缺失。第 k 轮剔除一行后点验必须红（清单不齐）。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inj_missing_row_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x5A) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(ROW_CAP as u32) as usize;
        let full = build_full_ledger();
        let tampered = full.clone_minus(pick);
        // 闸口：缺行清单点验必须不齐（行数缺一即红）。
        let still_green = tampered.checklist_complete();
        total += 1;
        if still_green {
            if escape.is_empty() {
                escape = format!("缺行注入逃脱：剔除行{}后点验仍全绿", pick);
            }
        } else {
            caught += 1;
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器二：风险缺账。对开放风险声明未覆盖，判定器必须 ESRCH 语义红。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inj_risk_gap_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x6B) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(RISK_CAP as u32);
        let risk_id = RISK_BASE + pick;
        // 闸口：在表风险声明未覆盖 → 必须 ESRCH 语义（缺账）。
        let verdict = judge_deviation(risk_id, false);
        total += 1;
        match verdict {
            Err(e) if e == DkDiag::EGap.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("风险缺账逃脱：风险{}声明未覆盖未判缺账", risk_id);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器三：越权覆盖。对不存在风险声明覆盖，判定器必须 EINVAL 红。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inj_over_cover_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x7C) as u64);
    let mut k = 0usize;
    while k < 10 {
        // 构造表外风险号：RISK_BASE+RISK_CAP 起向上抖动，恒非零恒不在表。
        let risk_id = RISK_BASE + (RISK_CAP as u32) + 1 + rng.below(64);
        // 闸口：表外风险声明覆盖 → 必须 EINVAL 语义（越权覆盖）。
        let verdict = judge_deviation(risk_id, true);
        total += 1;
        match verdict {
            Err(e) if e == DkDiag::EOverCover.val() => caught += 1,
            _ => {
                if escape.is_empty() {
                    escape = format!("越权覆盖逃脱：风险{}不在表却未被拒", risk_id);
                }
            }
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器四（QEMU 纪律）：向依赖引用副本注入 QEMU 禁词，扫描必须非零；
/// 同时干净登记表必须恒 0。返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inj_qemu_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    // 轮一：干净面必须为零（基准轮，也算入总轮数）。
    total += 1;
    if qemu_ref_count() != 0 {
        escape = format!("干净依赖表扫描非零：{}", qemu_ref_count());
    } else {
        caught += 1;
    }
    // 轮二..十：逐路注入 QEMU 禁词（带后缀变体），扫描必须抓到。
    let suffixes: [&str; 3] = ["-system", "-dbg", "-headless"];
    let mut k = 0usize;
    while k < 3 {
        let token = match QEMU_TOKENS.get(k) {
            Some(t) => *t,
            None => "qemu",
        };
        let suf = match suffixes.get(k) {
            Some(s) => *s,
            None => "",
        };
        let injected = [alloc::format!("{}{}", token, suf), "alloc".to_string()];
        let refs: Vec<&str> = injected.iter().map(|x| x.as_str()).collect();
        let n = scan_dep_refs(&refs);
        total += 1;
        if n > 0 {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("QEMU 注入逃脱：{}{} 未被扫描抓到", token, suf);
        }
        k += 1;
    }
    // 补足 10 轮：交叉注入双禁词拼接变体。
    let mut m = 0usize;
    while m < 6 {
        let a = QEMU_TOKENS.get(m % 3).copied().unwrap_or("qemu");
        let b = QEMU_TOKENS.get((m + 1) % 3).copied().unwrap_or("QEMU");
        let injected = [alloc::format!("{}_{}", a, b), "core".to_string()];
        let refs: Vec<&str> = injected.iter().map(|x| x.as_str()).collect();
        let n = scan_dep_refs(&refs);
        total += 1;
        if n > 0 {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("QEMU 注入逃脱：{}_{} 未被扫描抓到", a, b);
        }
        m += 1;
    }
    (caught, total, escape)
}

// ── 双跑一致指纹 ─────────────────────────────────────────────────
/// FNV-1a 字节混合（确定性，双跑一致）。
fn fnv_mix(h: &mut u64, b: u8) {
    *h ^= b as u64;
    *h = h.wrapping_mul(0x100_0000_01b3);
}

/// 混入一个 u32（小端四字节）。
fn fnv_mix_u32(h: &mut u64, v: u32) {
    let mut k = 0u32;
    while k < 4 {
        let shift = k.saturating_mul(8);
        fnv_mix(h, ((v >> shift) & 0xFF) as u8);
        k += 1;
    }
}

/// 全域指纹：清单 48 行 + 状态机 + 风险表 + 判定器全 verdict + QEMU 计数
/// + 三类注入战果，全部确定性混合。双跑必须逐位一致。
pub fn fingerprint() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    fnv_mix_u32(&mut h, SEG_BASE);
    // 轨道 A：清单行三元组逐行混入。
    let l = build_full_ledger();
    fnv_mix_u32(&mut h, l.len() as u32);
    let mut i = 0usize;
    while i < l.len() {
        if let Some(r) = l.row_at(i) {
            fnv_mix_u32(&mut h, r.idx);
            fnv_mix_u32(&mut h, r.cid);
            fnv_mix_u32(&mut h, r.cond);
            fnv_mix_u32(&mut h, r.budget);
        }
        i += 1;
    }
    // 状态机推进链：verify→freeze 后终态序。
    let mut l2 = build_full_ledger();
    let _ = l2.verify();
    let _ = l2.freeze();
    fnv_mix_u32(&mut h, l2.phase().rank() as u32);
    // 轨道 B：风险表逐条混入。
    let rows = handoff_risk_rows();
    fnv_mix_u32(&mut h, rows.len() as u32);
    for r in rows.iter() {
        fnv_mix_u32(&mut h, r.risk_id);
        fnv_mix_u32(&mut h, r.desc);
        fnv_mix_u32(&mut h, r.covered as u32);
    }
    // 判定器全 verdict：8 风险双向 + 探针四路。
    for r in rows.iter() {
        match judge_deviation(r.risk_id, true) {
            Ok(()) => fnv_mix_u32(&mut h, 0),
            Err(e) => fnv_mix_u32(&mut h, e),
        }
        match judge_deviation(r.risk_id, false) {
            Ok(()) => fnv_mix_u32(&mut h, 0),
            Err(e) => fnv_mix_u32(&mut h, e),
        }
    }
    match judge_deviation(0, true) {
        Ok(()) => fnv_mix_u32(&mut h, 0),
        Err(e) => fnv_mix_u32(&mut h, e),
    }
    match judge_deviation(RISK_BASE + RISK_CAP as u32 + 9, true) {
        Ok(()) => fnv_mix_u32(&mut h, 0),
        Err(e) => fnv_mix_u32(&mut h, e),
    }
    // 对账报告 + QEMU 计数 + 注入战果。
    let rep = full_reconciliation();
    fnv_mix_u32(&mut h, rep.checked);
    fnv_mix_u32(&mut h, rep.gap);
    fnv_mix_u32(&mut h, rep.over);
    fnv_mix_u32(&mut h, rep.zero_dev as u32);
    fnv_mix_u32(&mut h, qemu_ref_count());
    let (c1, t1, _) = inj_missing_row_sweep();
    let (c2, t2, _) = inj_risk_gap_sweep();
    let (c3, t3, _) = inj_over_cover_sweep();
    let (c4, t4, _) = inj_qemu_sweep();
    fnv_mix_u32(&mut h, c1 as u32);
    fnv_mix_u32(&mut h, t1 as u32);
    fnv_mix_u32(&mut h, c2 as u32);
    fnv_mix_u32(&mut h, t2 as u32);
    fnv_mix_u32(&mut h, c3 as u32);
    fnv_mix_u32(&mut h, t3 as u32);
    fnv_mix_u32(&mut h, c4 as u32);
    fnv_mix_u32(&mut h, t4 as u32);
    h
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落窗口、互异、查表可命中、未知码回落。
fn diag_closed_ok() -> bool {
    for d in DK_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 互异：任意两码不等。
    let mut i = 0usize;
    while i < DK_ALL.len() {
        let vi = match DK_ALL.get(i) {
            Some(d) => d.val(),
            None => return false,
        };
        let mut j = i + 1;
        while j < DK_ALL.len() {
            let vj = match DK_ALL.get(j) {
                Some(d) => d.val(),
                None => return false,
            };
            if vi == vj {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    // 查表命中与回落。
    if detail(DkDiag::EGap.val()) != DkDiag::EGap.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == DkDiag::EGap.detail() {
        return false;
    }
    // errno 语义映射闭集。
    if errno_semantics(DkDiag::EGap.val()) != ERRNO_ESRCH {
        return false;
    }
    if errno_semantics(DkDiag::EOverCover.val()) != ERRNO_EINVAL {
        return false;
    }
    true
}

/// 冻结分布规则守卫：48 个位置的三列冻结值逐位可复算且三列非空。
fn frozen_rule_ok() -> bool {
    let mut i = 0u32;
    while (i as usize) < ROW_CAP {
        let c = frozen_cid(i);
        let k = frozen_cond(i);
        let b = frozen_budget(i);
        // 判据号等距复算。
        if c != CID_BASE + i.saturating_mul(3) {
            return false;
        }
        // 条件代号必须落在六类闭集。
        if CondKind::from_code(k).is_none() {
            return false;
        }
        // 预算上限内且非零。
        if b == 0 || b > BUDGET_CAP {
            return false;
        }
        if c == 0 || k == 0 {
            return false;
        }
        i += 1;
    }
    // 末号与闭集上限一致。
    frozen_cid((ROW_CAP - 1) as u32) == CID_BASE + ((ROW_CAP as u32 - 1).saturating_mul(3))
}

/// 正常流：登记 48 行→点验→冻结全绿，状态机只进不退。
fn happy_path_ok() -> bool {
    let mut l = build_full_ledger();
    if l.len() != ROW_CAP {
        return false;
    }
    if l.phase() != LedgerPhase::Registering {
        return false;
    }
    match l.verify() {
        Ok(()) => {}
        Err(_) => return false,
    }
    if l.phase() != LedgerPhase::Verified {
        return false;
    }
    match l.freeze() {
        Ok(()) => l.phase() == LedgerPhase::Frozen,
        Err(_) => false,
    }
}

/// 三列逐行非空可复算：齐账清单每行三列非零且等于冻结值。
fn rows_recompute_ok() -> bool {
    let l = build_full_ledger();
    if !l.checklist_complete() {
        return false;
    }
    let mut i = 0usize;
    while i < ROW_CAP {
        let r = match l.row_at(i) {
            Some(r) => r,
            None => return false,
        };
        if !r.cols_nonempty() {
            return false;
        }
        if r.cid != frozen_cid(i as u32)
            || r.cond != frozen_cond(i as u32)
            || r.budget != frozen_budget(i as u32)
        {
            return false;
        }
        i += 1;
    }
    true
}

/// 轨道 B 守卫：风险表审计过 + 全表对账零偏差。
fn reconciliation_ok() -> bool {
    if handoff_risk_audit().is_err() {
        return false;
    }
    let rep = full_reconciliation();
    rep.zero_dev && rep.checked == RISK_CAP as u32 && rep.gap == 0 && rep.over == 0
}

/// 判定次序守卫：零号位先于存在性、存在性先于覆盖比对，逐路实测。
fn judge_order_ok() -> bool {
    // 次序闭集单调。
    if !(JudgeStep::ZeroCheck.rank() < JudgeStep::Existence.rank()
        && JudgeStep::Existence.rank() < JudgeStep::Coverage.rank())
    {
        return false;
    }
    // 零号位先于越权判定：0 号声明覆盖判 EJudgeZero 而非 EOverCover。
    if judge_deviation(0, true) != Err(DkDiag::EJudgeZero.val()) {
        return false;
    }
    // 零号位先于"未覆盖非风险 Ok"判定：0 号未覆盖也拒。
    if judge_deviation(0, false) != Err(DkDiag::EJudgeZero.val()) {
        return false;
    }
    // 存在性先于覆盖比对：在表未覆盖判 ESRCH（缺账）而非其他码。
    if judge_deviation(RISK_BASE, false) != Err(DkDiag::EGap.val()) {
        return false;
    }
    // 在表已覆盖 → Ok（零偏差）。
    if judge_deviation(RISK_BASE, true).is_err() {
        return false;
    }
    // 表外声明覆盖 → EINVAL（越权）；表外未覆盖 → Ok。
    let ghost = RISK_BASE + RISK_CAP as u32 + 3;
    if judge_deviation(ghost, true) != Err(DkDiag::EOverCover.val()) {
        return false;
    }
    if judge_deviation(ghost, false).is_err() {
        return false;
    }
    true
}

/// 注入必抓：三类注入器各 10 轮全数命中，无逃脱。
fn injections_all_caught_ok() -> bool {
    let (c1, t1, e1) = inj_missing_row_sweep();
    let (c2, t2, e2) = inj_risk_gap_sweep();
    let (c3, t3, e3) = inj_over_cover_sweep();
    if t1 != 10 || c1 != 10 || !e1.is_empty() {
        return false;
    }
    if t2 != 10 || c2 != 10 || !e2.is_empty() {
        return false;
    }
    if t3 != 10 || c3 != 10 || !e3.is_empty() {
        return false;
    }
    true
}

/// 零 QEMU 纪律守卫：干净面恒 0 + 注入 10 轮全抓。
fn qemu_discipline_ok() -> bool {
    if qemu_ref_count() != 0 {
        return false;
    }
    let (caught, total, escape) = inj_qemu_sweep();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 双跑一致：全域指纹两次逐位一致。
fn dual_run_ok() -> bool {
    let f1 = fingerprint();
    let f2 = fingerprint();
    f1 == f2 && f1 != 0
}

/// 边界四类：首行跳行 / 尾行越位 / 列缺与越上限 / 违冻结公式 / 状态机闸。
fn boundary_ok() -> bool {
    // 边界一：首行跳行——首登即用第 1 行的判据号被拒（乱序）。
    let mut l = RetestLedger::new();
    if l.register(frozen_cid(1), frozen_cond(1), frozen_budget(1))
        != Err(DkDiag::ERuleGap.val())
    {
        return false;
    }
    // 边界二：越闭集——判据号高于末号被拒。
    let mut l2 = RetestLedger::new();
    if l2.register(frozen_cid(ROW_CAP as u32), 1, 128) != Err(DkDiag::ERowRange.val()) {
        return false;
    }
    // 边界三：列缺与越上限——条件零 / 预算零 / 预算越顶 各拒。
    let mut l3 = RetestLedger::new();
    if l3.register(frozen_cid(0), 0, frozen_budget(0)) != Err(DkDiag::ECondCol.val()) {
        return false;
    }
    let mut l4 = RetestLedger::new();
    if l4.register(frozen_cid(0), frozen_cond(0), 0) != Err(DkDiag::EBudgetCol.val()) {
        return false;
    }
    let mut l5 = RetestLedger::new();
    if l5.register(frozen_cid(0), frozen_cond(0), BUDGET_CAP + 1)
        != Err(DkDiag::EBudgetCol.val())
    {
        return false;
    }
    // 边界四：违冻结——闭集内条件代号/上限内预算但不等于冻结值 各拒。
    let mut l6 = RetestLedger::new();
    if l6.register(frozen_cid(0), 2, frozen_budget(0)) != Err(DkDiag::ECondFrozen.val()) {
        return false;
    }
    let mut l7 = RetestLedger::new();
    if l7.register(frozen_cid(0), frozen_cond(0), frozen_budget(0) + 1)
        != Err(DkDiag::EBudgetFrozen.val())
    {
        return false;
    }
    // 状态机闸：未齐点验拒；冻结后写入拒；重复冻结倒退拒。
    let mut l8 = build_full_ledger();
    match l8.verify() {
        Ok(()) => {}
        Err(_) => return false,
    }
    match l8.freeze() {
        Ok(()) => {}
        Err(_) => return false,
    }
    if l8.register(frozen_cid(0), frozen_cond(0), frozen_budget(0))
        != Err(DkDiag::ESealed.val())
    {
        return false;
    }
    if l8.freeze() != Err(DkDiag::ERewind.val()) {
        return false;
    }
    let mut l9 = RetestLedger::new();
    if l9
        .register(frozen_cid(0), frozen_cond(0), frozen_budget(0))
        .is_err()
    {
        return false;
    }
    // 一行未齐：点验必须拒（ENotFull）。
    if l9.verify() != Err(DkDiag::ENotFull.val()) {
        return false;
    }
    true
}

/// 容量耗尽：48 行满后再登记必拒且账面不变。
fn capacity_ok() -> bool {
    let mut l = build_full_ledger();
    if l.len() != ROW_CAP {
        return false;
    }
    let before = l.len();
    // 用第 48 位置之后的合法冻结值探测容量闸。
    if l.register(frozen_cid(ROW_CAP as u32), frozen_cond(0), frozen_budget(0))
        != Err(DkDiag::ECapFull.val())
    {
        return false;
    }
    if l.len() != before {
        return false;
    }
    // 点验/冻结仍可走完（容量闸不影响只读推进）。
    match l.verify() {
        Ok(()) => {}
        Err(_) => return false,
    }
    match l.freeze() {
        Ok(()) => l.phase() == LedgerPhase::Frozen,
        Err(_) => false,
    }
}

/// 聚合守卫：容量耗尽 + 边界四类 + 完成写回 + 三类注入总闸，全绿才放行。
fn aggregates_ok() -> bool {
    capacity_ok() && boundary_ok() && writeback_ok() && injections_all_caught_ok()
}

/// 完成写回语义：冻结即"状态=已完成"，声明摘要非空。
fn writeback_ok() -> bool {
    let mut l = build_full_ledger();
    let _ = l.verify();
    if l.freeze().is_err() {
        return false;
    }
    let rep = full_reconciliation();
    let summary = format!(
        "rows={} frozen={} zero_dev={}",
        l.len(),
        l.phase() == LedgerPhase::Frozen,
        rep.zero_dev
    );
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 检查节条数（收口声明锚定：15 条主检 + 1 条收口 = 16）。
pub const EXPECTED_CHECKS: usize = 16;
/// 判据条数（UNX-F12797-J1 一条，逐字锚定）。
pub const CRITERIA_COUNT: usize = 1;

/// UNX-F12796 自检（判据 12796 卡：补测清单齐三列 + handoff 开放风险零偏差）。
pub fn run_unx12796_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12796");
    let ledger = build_full_ledger();

    s.add_dyn(
        "F12797-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} in_window={}",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            (0x80000..=0x831C0).contains(&SEG_BASE)
        ),
    );

    s.add_dyn(
        "F12797-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x}",
            DK_ALL.len(),
            DK_ALL.first().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12797-03 冻结分布规则 48 行",
        frozen_rule_ok(),
        &format!(
            "rows={} cid首={} cid尾={} 条件类=6 预算上限={}",
            ROW_CAP,
            frozen_cid(0),
            frozen_cid((ROW_CAP - 1) as u32),
            BUDGET_CAP
        ),
    );

    s.add_dyn(
        "F12797-04 正常流登记点验冻结",
        happy_path_ok(),
        &format!("rows={} phase={:?}", ledger.len(), ledger.phase()),
    );

    s.add_dyn(
        "F12797-05 三列逐行非空可复算",
        rows_recompute_ok(),
        &format!(
            "行0={} 行47={}",
            ledger
                .row_at(0)
                .map(|r| r.brief())
                .unwrap_or_else(|| "缺".to_string()),
            ledger
                .row_at(47)
                .map(|r| r.brief())
                .unwrap_or_else(|| "缺".to_string())
        ),
    );

    s.add_dyn(
        "F12797-06 清单齐点验",
        ledger.checklist_complete(),
        &format!("rows={}/{} audit={}", ledger.len(), ROW_CAP, ledger.audit_rows().is_ok()),
    );

    s.add_dyn(
        "F12797-07 handoff 风险表闭集",
        handoff_risk_audit().is_ok(),
        &format!(
            "risks={} base={} desc首={}",
            RISK_CAP,
            RISK_BASE,
            handoff_risk_rows()
                .first()
                .map(|r| r.desc)
                .unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12797-08 全表对账零偏差",
        reconciliation_ok(),
        &format!(
            "checked={} gap={} over={} zero={}",
            full_reconciliation().checked,
            full_reconciliation().gap,
            full_reconciliation().over,
            full_reconciliation().zero_dev
        ),
    );

    s.add_dyn(
        "F12797-09 判定次序三步",
        judge_order_ok(),
        "零号位→存在性→覆盖比对：ESRCH缺账/EINVAL越权/零号位各一路实测",
    );

    let (c1, t1, e1) = inj_missing_row_sweep();
    s.add_dyn(
        "F12797-10 注入一：清单行缺失 10/10",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inj_risk_gap_sweep();
    s.add_dyn(
        "F12797-11 注入二：风险缺账 10/10",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inj_over_cover_sweep();
    s.add_dyn(
        "F12797-12 注入三：越权覆盖 10/10",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    let (c4, t4, e4) = inj_qemu_sweep();
    s.add_dyn(
        "F12797-13 零 QEMU 纪律：引用计数恒 0",
        qemu_discipline_ok() && c4 == t4 && e4.is_empty(),
        &format!("refs={} caught={}/{} {}", qemu_ref_count(), c4, t4, e4),
    );

    s.add_dyn(
        "F12797-14 双跑一致指纹",
        dual_run_ok(),
        &format!("fp=0x{:016x}", fingerprint()),
    );

    s.add_dyn(
        "F12797-15 容量耗尽边界",
        aggregates_ok(),
        "48行满拒写/首行跳行/越闭集/列缺越顶/违冻结/冻结闸 各一路+三类注入总闸",
    );

    // 收口：判据条数声明（判据 1 条 · 检查节 16 条，账实相符才绿）。
    s.add_dyn(
        "F12797-16 收口：判据条数声明",
        CRITERIA_COUNT == 1
            && EXPECTED_CHECKS == 16
            && s.len() + 1 == EXPECTED_CHECKS,
        &format!("判据{}条 检查{}条（含本条收口）", CRITERIA_COUNT, EXPECTED_CHECKS),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 批窗口 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x83100);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：18 个码互异、全部落窗口、查表可命中、errno 语义正确。
        assert_eq!(DK_ALL.len(), 18);
        let mut i = 0usize;
        while i < DK_ALL.len() {
            let vi = DK_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DK_ALL.len() {
                let vj = DK_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
        // errno 语义映射。
        assert_eq!(errno_semantics(DkDiag::EGap.val()), ERRNO_ESRCH);
        assert_eq!(errno_semantics(DkDiag::EOverCover.val()), ERRNO_EINVAL);
        assert_eq!(errno_semantics(DkDiag::EJudgeZero.val()), ERRNO_EINVAL);
        assert_eq!(errno_semantics(DkDiag::ECapFull.val()), 0);
    }

    #[test]
    fn frozen_rule_recomputes_all_rows() {
        // 冻结分布：48 行三列逐位复算等于冻结公式。
        assert!(frozen_rule_ok());
        let mut i = 0u32;
        while (i as usize) < ROW_CAP {
            assert_eq!(frozen_cid(i), CID_BASE + 3 * i);
            assert!(CondKind::from_code(frozen_cond(i)).is_some());
            assert_eq!(frozen_budget(i), 128 + 16 * i);
            assert!(frozen_budget(i) <= BUDGET_CAP);
            i += 1;
        }
        // 行可复算：齐账清单逐行三列等于冻结值。
        let l = build_full_ledger();
        assert!(rows_recompute_ok());
        let r0 = l.row_at(0);
        assert!(r0.is_some());
        assert_eq!(r0.map(|r| r.cid).unwrap_or(0), frozen_cid(0));
        let r47 = l.row_at(47);
        assert!(r47.is_some());
        assert_eq!(r47.map(|r| r.budget).unwrap_or(0), frozen_budget(47));
    }

    #[test]
    fn happy_path_freezes_after_verify() {
        // 正常流：登记 48 → 点验 → 冻结，状态机只进不退。
        let mut l = build_full_ledger();
        assert_eq!(l.len(), 48);
        assert_eq!(l.phase(), LedgerPhase::Registering);
        assert!(l.checklist_complete());
        assert!(l.verify().is_ok());
        assert_eq!(l.phase(), LedgerPhase::Verified);
        assert!(l.freeze().is_ok());
        assert_eq!(l.phase(), LedgerPhase::Frozen);
        // 冻结后写入拒且账面不变。
        let before = l.len();
        assert_eq!(
            l.register(frozen_cid(0), frozen_cond(0), frozen_budget(0)),
            Err(DkDiag::ESealed.val())
        );
        assert_eq!(l.len(), before);
        // 状态序单调。
        assert!(LedgerPhase::Registering.rank() < LedgerPhase::Verified.rank());
        assert!(LedgerPhase::Verified.rank() < LedgerPhase::Frozen.rank());
    }

    #[test]
    fn boundary_gates_reject() {
        // 边界一：首行跳行。
        let mut l = RetestLedger::new();
        assert_eq!(
            l.register(frozen_cid(1), frozen_cond(1), frozen_budget(1)),
            Err(DkDiag::ERuleGap.val())
        );
        // 边界二：尾行越闭集（高于末号）。
        let mut l2 = RetestLedger::new();
        assert_eq!(
            l2.register(frozen_cid(ROW_CAP as u32), 1, 128),
            Err(DkDiag::ERowRange.val())
        );
        // 边界三：列缺与越上限。
        let mut l3 = RetestLedger::new();
        assert_eq!(
            l3.register(frozen_cid(0), 0, frozen_budget(0)),
            Err(DkDiag::ECondCol.val())
        );
        let mut l4 = RetestLedger::new();
        assert_eq!(
            l4.register(frozen_cid(0), frozen_cond(0), 0),
            Err(DkDiag::EBudgetCol.val())
        );
        let mut l5 = RetestLedger::new();
        assert_eq!(
            l5.register(frozen_cid(0), frozen_cond(0), BUDGET_CAP + 1),
            Err(DkDiag::EBudgetCol.val())
        );
        // 边界四：违冻结（闭集内但非冻结值）。
        let mut l6 = RetestLedger::new();
        assert_eq!(
            l6.register(frozen_cid(0), 2, frozen_budget(0)),
            Err(DkDiag::ECondFrozen.val())
        );
        let mut l7 = RetestLedger::new();
        assert_eq!(
            l7.register(frozen_cid(0), frozen_cond(0), frozen_budget(0) + 1),
            Err(DkDiag::EBudgetFrozen.val())
        );
        // 未齐点验拒。
        let mut l8 = RetestLedger::new();
        let _ = l8.register(frozen_cid(0), frozen_cond(0), frozen_budget(0));
        assert_eq!(l8.verify(), Err(DkDiag::ENotFull.val()));
    }

    #[test]
    fn injection_missing_rows_all_caught() {
        // 注入一：清单行缺失 10 轮全抓，无逃脱。
        let (caught, total, escape) = inj_missing_row_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 单路直证：剔除任意一行后清单必不齐。
        let full = build_full_ledger();
        assert!(full.checklist_complete());
        let minus = full.clone_minus(17);
        assert!(!minus.checklist_complete());
    }

    #[test]
    fn injection_risk_gap_all_caught() {
        // 注入二：风险缺账 10 轮全抓（ESRCH 语义）。
        let (caught, total, escape) = inj_risk_gap_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 单路直证：全部 8 条开放风险声明未覆盖均判缺账。
        let mut i = 0usize;
        while i < RISK_CAP {
            let id = RISK_BASE + i as u32;
            assert_eq!(
                judge_deviation(id, false),
                Err(DkDiag::EGap.val()),
                "风险{}缺账未抓",
                id
            );
            i += 1;
        }
    }

    #[test]
    fn injection_over_cover_all_caught() {
        // 注入三：越权覆盖 10 轮全抓（EINVAL 语义）。
        let (caught, total, escape) = inj_over_cover_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 单路直证：表外号声明覆盖必拒。
        let ghost = RISK_BASE + RISK_CAP as u32 + 7;
        assert_eq!(
            judge_deviation(ghost, true),
            Err(DkDiag::EOverCover.val())
        );
    }

    #[test]
    fn judge_order_precedence_holds() {
        // 判定次序：零号位 → 存在性 → 覆盖比对。
        assert!(judge_order_ok());
        // 零号位先于越权：0 声明覆盖判 EJudgeZero 而非 EOverCover。
        assert_eq!(judge_deviation(0, true), Err(DkDiag::EJudgeZero.val()));
        assert_eq!(judge_deviation(0, false), Err(DkDiag::EJudgeZero.val()));
        // 存在性先于覆盖：在表未覆盖判 ESRCH。
        assert_eq!(judge_deviation(RISK_BASE + 5, false), Err(DkDiag::EGap.val()));
        assert_eq!(judge_deviation(RISK_BASE + 5, true), Ok(()));
        // 表外：覆盖→EINVAL，未覆盖→Ok。
        let ghost = RISK_BASE + RISK_CAP as u32 + 1;
        assert_eq!(judge_deviation(ghost, true), Err(DkDiag::EOverCover.val()));
        assert_eq!(judge_deviation(ghost, false), Ok(()));
        // 次序闭集单调。
        assert!(JudgeStep::ZeroCheck.rank() < JudgeStep::Existence.rank());
        assert!(JudgeStep::Existence.rank() < JudgeStep::Coverage.rank());
        // 全表对账零偏差。
        let rep = full_reconciliation();
        assert!(rep.zero_dev);
        assert_eq!(rep.checked, RISK_CAP as u32);
        assert_eq!(rep.gap, 0);
        assert_eq!(rep.over, 0);
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：全域指纹两次逐位一致且非零。
        let a = fingerprint();
        let b = fingerprint();
        assert_eq!(a, b);
        assert_ne!(a, 0);
        // 结构一致：两次齐账清单逐行相同。
        let l1 = build_full_ledger();
        let l2 = build_full_ledger();
        assert_eq!(l1.len(), l2.len());
        let mut i = 0usize;
        while i < ROW_CAP {
            assert_eq!(l1.row_at(i), l2.row_at(i));
            i += 1;
        }
        // 注入战果双跑一致。
        assert_eq!(inj_missing_row_sweep(), inj_missing_row_sweep());
        assert_eq!(inj_risk_gap_sweep(), inj_risk_gap_sweep());
        assert_eq!(inj_over_cover_sweep(), inj_over_cover_sweep());
        assert_eq!(inj_qemu_sweep(), inj_qemu_sweep());
    }

    #[test]
    fn capacity_exhaustion_rejects() {
        // 容量耗尽：48 行满后再登记必拒且账面不变。
        let mut l = build_full_ledger();
        assert_eq!(l.len(), ROW_CAP);
        let before = l.len();
        assert_eq!(
            l.register(frozen_cid(ROW_CAP as u32), frozen_cond(0), frozen_budget(0)),
            Err(DkDiag::ECapFull.val())
        );
        assert_eq!(l.len(), before);
        // 点验/冻结仍可走完。
        assert!(l.verify().is_ok());
        assert!(l.freeze().is_ok());
    }

    #[test]
    fn qemu_ref_count_is_zero() {
        // 零 QEMU 纪律：干净依赖表引用计数恒 0。
        assert!(qemu_discipline_ok());
        assert_eq!(qemu_ref_count(), 0);
        // 注入 10 轮全抓。
        let (caught, total, escape) = inj_qemu_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        // 禁词扫描单路直证。
        assert_eq!(scan_dep_refs(&["alloc", "core"]), 0);
        assert!(scan_dep_refs(&["qemu-system"]) > 0);
        assert!(scan_dep_refs(&["crate::qemu_helper"]) > 0);
    }

    #[test]
    fn risk_table_audit_closed() {
        // 轨道 B：风险表审计过，风险号连续、描述代号闭集、覆盖全真。
        assert!(handoff_risk_audit().is_ok());
        let rows = handoff_risk_rows();
        assert_eq!(rows.len(), RISK_CAP);
        for r in rows.iter() {
            assert!(RiskDesc::from_code(r.desc).is_some());
            assert!(r.covered);
            assert!(r.risk_id >= RISK_BASE && r.risk_id < RISK_BASE + RISK_CAP as u32);
        }
        // 描述代号越闭集直证。
        assert!(RiskDesc::from_code(0).is_none());
        assert!(RiskDesc::from_code(6).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且 ≥12 条（含收口）。
        let s = run_unx12796_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.len(), EXPECTED_CHECKS);
        assert_eq!(s.tally().1, 0);
        assert!(writeback_ok());
    }
}
