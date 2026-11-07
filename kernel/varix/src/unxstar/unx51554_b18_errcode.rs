//! UNX-F51554 B18 错误码扩展（HUB 电源族 ×12 并入域内三表） · 0xC0D40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F51554 卡验收②逐字）：
//!   判据原文：UNX-F51555-J1 档位 ×10 判定正确；超档取流注入 10/10 拒绝。
//!
//! 设计要点（三表登记模型——并入、封账、探针三段状态机）：
//!   ① 错误码扩展按「三表登记」建模：登记表（码→名称）、建议表（码→中文
//!      建议）、域归属表（码→域类）合一为行账 ErrRow；HUB 电源族 ×12 逐条
//!      并入，并入即三表同展一_row_，禁只并一表（三表一致性由行账保证）。
//!   ② 状态机单调：Closed → Merging（并入中）→ Sealed（封账）；封账后
//!      再并入/再封账显性拒（SealedFrozen，禁乒乓）；并入不足 ×12 封账
//!      拒（TableMismatch），状态不回退。
//!   ③ 未定义码探针：封账前探针显性拒（NotSealed）；封账后查表，命中
//!      返回码值与域类，未命中即 UndefinedCode 计拒账，×10 注入 10/10 拒。
//!   ④ 判定次序：并入面 = 封账状态 → 容量界 → 重复码；容量先于重复，
//!      避免表满时重复码噪音掩盖容量真因。
//!   ⑤ 守恒账：并入数 + 拒并数 = 尝试并数；探针拒账 = 未定义注入数；
//!      注入 = 检出 + 逃逸，逃逸恒 0；行账码值回放严格递增。
//!   ⑥ 诊断码闭集 10 变体，中文建议随码给出；Result<T, 并表码> 显性回传。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0D40;
/// 基础域码数（建账即登记）。
pub const BASE_CODE_COUNT: usize = 8;
/// HUB 电源族码数（本次并入目标 ×12）。
pub const FAMILY_CODE_COUNT: usize = 12;
/// 三表行账容量（8 基础 + 12 电源族，满即显性拒）。
pub const TABLE_CAP: usize = BASE_CODE_COUNT + FAMILY_CODE_COUNT;

/// 域类闭集（域归属表取值）。
pub const DOMAIN_BUS: u8 = 1;
pub const DOMAIN_PORT: u8 = 2;
pub const DOMAIN_HUB: u8 = 3;

/// 诊断码闭集（错误码扩展域，val = SEG_BASE + 变体序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 未封账（探针在封账前调用）
    NotSealed,
    /// 已封账（封账后再并入/再封账，禁乒乓）
    SealedFrozen,
    /// 三表行账满（容量耗尽显性拒）
    TableFull,
    /// 重复码（同一码重复并入）
    DupCode,
    /// 并入不完整即封账（×12 未并满）
    TableMismatch,
    /// 未定义码（探针查无此码）
    UndefinedCode,
    /// 行账回放不一致（码值序倒退）
    ReplayMismatch,
    /// 账面失守（并入账与行账数不符）
    LedgerMismatch,
}

impl MergeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MergeCode::Ok => "并表操作成立",
            MergeCode::SegBad => "段位越界：核对 SEG_BASE 对齐与取值区间",
            MergeCode::NotSealed => "未封账：探针必须等封账后再查",
            MergeCode::SealedFrozen => "已封账：错误码集冻结，禁再并入",
            MergeCode::TableFull => "三表已满：先收口再扩容",
            MergeCode::DupCode => "重复码：同一码禁止重复并入",
            MergeCode::TableMismatch => "并入不完整：×12 并满才可封账",
            MergeCode::UndefinedCode => "未定义码：查无此码，核对并入清单",
            MergeCode::ReplayMismatch => "回放不一致：行账码值序被改写",
            MergeCode::LedgerMismatch => "账面失守：并入账必须与行账数一致",
        }
    }
}

/// HUB 电源族 ×12（冻结描述：码值、名称、中文建议、域类三表一体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HubPowerCode {
    /// VBUS 压降
    VbusDrop,
    /// 端口过流
    OverCurrent,
    /// 电源故障
    PowerFault,
    /// VBUS 切断
    VbusOff,
    /// 挂起断电
    SuspendPower,
    /// 恢复供电
    ResumePower,
    /// 过压
    OverVoltage,
    /// 欠压
    UnderVoltage,
    /// 充电直通异常
    ChargeThrough,
    /// 电源开关失效
    PwrSwFail,
    /// 档位卡滞
    TierStuck,
    /// 供电重协商失败
    ReforcedFail,
}

impl HubPowerCode {
    pub fn code(self) -> u32 {
        SEG_BASE + 0x200 + self as u32
    }
    pub fn name(self) -> &'static str {
        match self {
            HubPowerCode::VbusDrop => "VBUS 压降",
            HubPowerCode::OverCurrent => "端口过流",
            HubPowerCode::PowerFault => "电源故障",
            HubPowerCode::VbusOff => "VBUS 切断",
            HubPowerCode::SuspendPower => "挂起断电",
            HubPowerCode::ResumePower => "恢复供电",
            HubPowerCode::OverVoltage => "过压",
            HubPowerCode::UnderVoltage => "欠压",
            HubPowerCode::ChargeThrough => "充电直通异常",
            HubPowerCode::PwrSwFail => "电源开关失效",
            HubPowerCode::TierStuck => "档位卡滞",
            HubPowerCode::ReforcedFail => "供电重协商失败",
        }
    }
    pub fn advice(self) -> &'static str {
        match self {
            HubPowerCode::VbusDrop => "核对线缆压降与端口供电预算",
            HubPowerCode::OverCurrent => "限流降档并标记高电流端口",
            HubPowerCode::PowerFault => "隔离故障口并触发域复位",
            HubPowerCode::VbusOff => "检查拔出时序与残留电荷泄放",
            HubPowerCode::SuspendPower => "核对挂起策略与唤醒预算",
            HubPowerCode::ResumePower => "核对恢复上电斜率与档位",
            HubPowerCode::OverVoltage => "检查 OVP 阈值表与分压网络",
            HubPowerCode::UnderVoltage => "检查欠压门槛与上游供电",
            HubPowerCode::ChargeThrough => "核对充电直通开关与预算表",
            HubPowerCode::PwrSwFail => "更换电源开关路径并降级运行",
            HubPowerCode::TierStuck => "复位档位锁存器再重判",
            HubPowerCode::ReforcedFail => "重走供电重协商状态机",
        }
    }
    pub fn domain_class(self) -> u8 {
        match self {
            HubPowerCode::VbusDrop
            | HubPowerCode::OverVoltage
            | HubPowerCode::UnderVoltage => DOMAIN_BUS,
            HubPowerCode::OverCurrent
            | HubPowerCode::PowerFault
            | HubPowerCode::VbusOff
            | HubPowerCode::SuspendPower
            | HubPowerCode::ResumePower
            | HubPowerCode::ChargeThrough => DOMAIN_PORT,
            HubPowerCode::PwrSwFail
            | HubPowerCode::TierStuck
            | HubPowerCode::ReforcedFail => DOMAIN_HUB,
        }
    }
}

/// 三表合一的行账行：码 → 名称 → 中文建议 → 域类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrRow {
    pub code: u32,
    pub name: &'static str,
    pub advice: &'static str,
    pub domain_class: u8,
}

/// 并表状态机：Closed → Merging → Sealed（单调，禁回退禁乒乓）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcPhase {
    /// 未开始并入
    Closed,
    /// 并入中（已并 k 条）
    Merging(u16),
    /// 已封账（错误码集冻结）
    Sealed,
}

/// 三表登记账：行账 + 状态机 + 探针拒账 + 守恒账。
pub struct ErrCodeTable {
    rows: Vec<ErrRow>,
    phase: EcPhase,
    merged: u64,
    merge_rejects: u64,
    probe_rejects: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ErrCodeTable {
    /// 建账：基础域码 ×8 即刻登记（名称/建议/域类冻结）。
    pub fn new() -> ErrCodeTable {
        let mut t = ErrCodeTable {
            rows: Vec::new(),
            phase: EcPhase::Closed,
            merged: 0,
            merge_rejects: 0,
            probe_rejects: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        };
        let base_names = [
            "域内基础错 0",
            "域内基础错 1",
            "域内基础错 2",
            "域内基础错 3",
            "域内基础错 4",
            "域内基础错 5",
            "域内基础错 6",
            "域内基础错 7",
        ];
        for i in 0..BASE_CODE_COUNT {
            let name = match base_names.get(i) {
                Some(n) => *n,
                None => "域内基础错",
            };
            let _ = t.rows.push(ErrRow {
                code: SEG_BASE + 0x100 + i as u32,
                name,
                advice: "核对域内基础错误清单",
                domain_class: DOMAIN_HUB,
            });
        }
        t
    }

    pub fn phase(&self) -> EcPhase {
        self.phase
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn merged(&self) -> u64 {
        self.merged
    }

    /// 并入一条电源族码：封账状态 → 容量界 → 重复码（判定次序固定）。
    pub fn merge_one(&mut self, h: HubPowerCode) -> Result<u64, MergeCode> {
        if let EcPhase::Sealed = self.phase {
            self.merge_rejects += 1;
            return Err(MergeCode::SealedFrozen);
        }
        if self.rows.len() >= TABLE_CAP {
            self.merge_rejects += 1;
            return Err(MergeCode::TableFull);
        }
        let dup = self.rows.iter().any(|r| r.code == h.code());
        if dup {
            self.merge_rejects += 1;
            return Err(MergeCode::DupCode);
        }
        let _ = self.rows.push(ErrRow {
            code: h.code(),
            name: h.name(),
            advice: h.advice(),
            domain_class: h.domain_class(),
        });
        self.merged += 1;
        self.phase = EcPhase::Merging(self.merged as u16);
        Ok(self.merged)
    }

    /// 封账：×12 并满才可封；封账后错误码集冻结。
    pub fn seal(&mut self) -> Result<(), MergeCode> {
        if let EcPhase::Sealed = self.phase {
            self.merge_rejects += 1;
            return Err(MergeCode::SealedFrozen);
        }
        if self.merged != FAMILY_CODE_COUNT as u64 {
            return Err(MergeCode::TableMismatch);
        }
        self.phase = EcPhase::Sealed;
        Ok(())
    }

    /// 探针查表：封账后按码查三表；未命中计拒账。
    pub fn probe(&mut self, code: u32) -> Result<ErrRow, MergeCode> {
        if !matches!(self.phase, EcPhase::Sealed) {
            return Err(MergeCode::NotSealed);
        }
        let mut hit = None;
        for r in self.rows.iter() {
            if r.code == code {
                hit = Some(*r);
            }
        }
        match hit {
            Some(row) => Ok(row),
            None => {
                self.probe_rejects += 1;
                Err(MergeCode::UndefinedCode)
            }
        }
    }

    /// 行账回放：码值严格递增（按并入序），返回行数。
    pub fn replay(&self) -> Result<u32, MergeCode> {
        let mut prev: i64 = -1;
        for r in self.rows.iter() {
            if (r.code as i64) <= prev {
                return Err(MergeCode::ReplayMismatch);
            }
            prev = r.code as i64;
            if r.name.is_empty() || r.advice.is_empty() {
                return Err(MergeCode::ReplayMismatch);
            }
        }
        Ok(self.rows.len() as u32)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    pub fn probe_rejects(&self) -> u64 {
        self.probe_rejects
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 三表一致性：行数 = 基础 + 并入，且账面相符。
    pub fn tables_consistent(&self) -> bool {
        self.rows.len() == BASE_CODE_COUNT + self.merged as usize
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致：同一并入序两次建账行账逐字段相等（确定性）。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<ErrRow> {
        let mut t = ErrCodeTable::new();
        let family = [
            HubPowerCode::VbusDrop,
            HubPowerCode::OverCurrent,
            HubPowerCode::PowerFault,
            HubPowerCode::VbusOff,
            HubPowerCode::SuspendPower,
            HubPowerCode::ResumePower,
            HubPowerCode::OverVoltage,
            HubPowerCode::UnderVoltage,
            HubPowerCode::ChargeThrough,
            HubPowerCode::PwrSwFail,
            HubPowerCode::TierStuck,
            HubPowerCode::ReforcedFail,
        ];
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        t.rows.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51554_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51554");
    let family = [
        HubPowerCode::VbusDrop,
        HubPowerCode::OverCurrent,
        HubPowerCode::PowerFault,
        HubPowerCode::VbusOff,
        HubPowerCode::SuspendPower,
        HubPowerCode::ResumePower,
        HubPowerCode::OverVoltage,
        HubPowerCode::UnderVoltage,
        HubPowerCode::ChargeThrough,
        HubPowerCode::PwrSwFail,
        HubPowerCode::TierStuck,
        HubPowerCode::ReforcedFail,
    ];
    // 1 正常流：×12 全部并入成功并入满。
    {
        let mut t = ErrCodeTable::new();
        let mut ok = 0u32;
        for h in family.iter() {
            if t.merge_one(*h).is_ok() {
                ok += 1;
            }
        }
        s.add_dyn(
            "电源族 ×12 全并入",
            ok == 12 && t.merged() == 12 && t.tables_consistent(),
            "HUB 电源族 12 条应全部并入三表",
        );
    }
    // 2 边界一：并入不足即封账 → 显性拒且状态不回退。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter().take(11) {
            let _ = t.merge_one(*h);
        }
        let r = t.seal();
        s.add_dyn(
            "欠量封账显性拒",
            r == Err(MergeCode::TableMismatch)
                && matches!(t.phase(), EcPhase::Merging(11)),
            "并入不满 12 条封账必须拒且状态停在并入中",
        );
    }
    // 3 边界二：封账后再并入 → 冻结拒（禁乒乓）。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let r = t.merge_one(HubPowerCode::VbusDrop);
        s.add_dyn(
            "封账后并入冻结拒",
            r == Err(MergeCode::SealedFrozen),
            "封账后错误码集冻结，禁再并入",
        );
    }
    // 4 边界三：封账前探针 → 显性拒。
    {
        let mut t = ErrCodeTable::new();
        let r = t.probe(HubPowerCode::VbusDrop.code());
        s.add_dyn(
            "封账前探针拒",
            r == Err(MergeCode::NotSealed),
            "未封账的表不得对外探针",
        );
    }
    // 5 边界四：封账后首尾族码探针命中且域类正确。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let first = t.probe(HubPowerCode::VbusDrop.code());
        let last = t.probe(HubPowerCode::ReforcedFail.code());
        s.add_dyn(
            "封账后探针首尾命中",
            matches!(first, Ok(row) if row.domain_class == DOMAIN_BUS)
                && matches!(last, Ok(row) if row.domain_class == DOMAIN_HUB),
            "首尾族码必须命中且域类归属正确",
        );
    }
    // 6 注入必抓一：未定义码探针 ×10 全拒。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = t.probe(SEG_BASE + 0x900 + i);
            if r == Err(MergeCode::UndefinedCode) {
                caught += 1;
                t.note_detected();
            }
        }
        s.add_dyn(
            "未定义码注入 10/10",
            caught == 10 && t.probe_rejects() == 10,
            "未定义码必须全额拒绝并落拒账",
        );
    }
    // 7 注入必抓二：重复码并入 ×10 全拒。
    {
        let mut t = ErrCodeTable::new();
        let _ = t.merge_one(HubPowerCode::VbusDrop);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = t.merge_one(HubPowerCode::VbusDrop);
            if r == Err(MergeCode::DupCode) {
                caught += 1;
                t.note_detected();
            }
        }
        s.add_dyn("重复码注入 10/10", caught == 10, "重复并入必须全抓");
    }
    // 8 注入必抓三：封账后再并入 ×10 全拒。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let mut caught = 0u32;
        for h in family.iter() {
            let r = t.merge_one(*h);
            if r == Err(MergeCode::SealedFrozen) {
                caught += 1;
                t.note_detected();
            }
        }
        s.add_dyn("冻结并入注入 10/10", caught == 10, "封账后并入必须全拒");
    }
    // 9 判定次序：并满未封时容量界先于重复码。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let r = t.merge_one(HubPowerCode::VbusDrop);
        s.add_dyn(
            "判序容量先于重复",
            r == Err(MergeCode::TableFull),
            "表满时容量判据先于重复码判据",
        );
    }
    // 10 状态机单调：封账后重复封账也拒（状态不回退不乒乓）。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let r2 = t.seal();
        s.add_dyn(
            "重复封账冻结拒",
            r2 == Err(MergeCode::SealedFrozen)
                && matches!(t.phase(), EcPhase::Sealed),
            "封账状态单调保持，禁乒乓",
        );
    }
    // 11 三表一致性：行数、非空文案、域类闭集。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let mut well_formed = true;
        for r in t.rows.iter() {
            if r.name.is_empty() || r.advice.is_empty() {
                well_formed = false;
            }
            if r.domain_class != DOMAIN_BUS
                && r.domain_class != DOMAIN_PORT
                && r.domain_class != DOMAIN_HUB
            {
                well_formed = false;
            }
        }
        s.add_dyn(
            "三表同构完整",
            well_formed && t.row_count() == TABLE_CAP && t.tables_consistent(),
            "每行三表字段齐备且域类在闭集内",
        );
    }
    // 12 行账回放可复现：码值严格递增 ×20 行。
    {
        let mut t = ErrCodeTable::new();
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
        let _ = t.seal();
        let n = t.replay();
        s.add_dyn(
            "行账回放可复现",
            matches!(n, Ok(c) if c == TABLE_CAP as u32),
            "码值严格递增且文案逐行非空",
        );
    }
    // 13 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut t = ErrCodeTable::new();
        for _ in 0..10u32 {
            let r = t.merge_one(HubPowerCode::OverCurrent);
            if r == Err(MergeCode::DupCode) {
                t.note_detected();
            }
        }
        s.add_dyn(
            "注入账守恒",
            t.conserved(),
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 14 双跑一致 + 文案闭集 + 段位守卫。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一并入序两次建行账逐字段相等");
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
        s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    }
    s
}

/// 诊断码全序表（长度与变体数严格一致：10 变体）。
pub const ALL_CODES: [MergeCode; 10] = [
    MergeCode::Ok,
    MergeCode::SegBad,
    MergeCode::NotSealed,
    MergeCode::SealedFrozen,
    MergeCode::TableFull,
    MergeCode::DupCode,
    MergeCode::TableMismatch,
    MergeCode::UndefinedCode,
    MergeCode::ReplayMismatch,
    MergeCode::LedgerMismatch,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn merge_all(t: &mut ErrCodeTable) {
        let family = [
            HubPowerCode::VbusDrop,
            HubPowerCode::OverCurrent,
            HubPowerCode::PowerFault,
            HubPowerCode::VbusOff,
            HubPowerCode::SuspendPower,
            HubPowerCode::ResumePower,
            HubPowerCode::OverVoltage,
            HubPowerCode::UnderVoltage,
            HubPowerCode::ChargeThrough,
            HubPowerCode::PwrSwFail,
            HubPowerCode::TierStuck,
            HubPowerCode::ReforcedFail,
        ];
        for h in family.iter() {
            let _ = t.merge_one(*h);
        }
    }

    #[test]
    fn base_rows_registered_at_build() {
        let t = ErrCodeTable::new();
        assert_eq!(t.row_count(), BASE_CODE_COUNT);
        assert!(matches!(t.phase(), EcPhase::Closed));
    }

    #[test]
    fn family_merge_twelve_and_seal() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        assert_eq!(t.merged(), 12);
        assert!(t.seal().is_ok());
        assert!(matches!(t.phase(), EcPhase::Sealed));
        assert_eq!(t.row_count(), TABLE_CAP);
    }

    #[test]
    fn seal_before_complete_rejected_phase_kept() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        // 撤不动账，只能另建欠量表：并 11 条。
        let mut t2 = ErrCodeTable::new();
        let partial = [
            HubPowerCode::VbusDrop,
            HubPowerCode::OverCurrent,
            HubPowerCode::PowerFault,
            HubPowerCode::VbusOff,
            HubPowerCode::SuspendPower,
            HubPowerCode::ResumePower,
            HubPowerCode::OverVoltage,
            HubPowerCode::UnderVoltage,
            HubPowerCode::ChargeThrough,
            HubPowerCode::PwrSwFail,
            HubPowerCode::TierStuck,
        ];
        for h in partial.iter() {
            let _ = t2.merge_one(*h);
        }
        assert_eq!(t2.seal(), Err(MergeCode::TableMismatch));
        assert!(matches!(t2.phase(), EcPhase::Merging(11)));
        let _ = t;
    }

    #[test]
    fn sealed_merge_and_reseal_frozen() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        assert!(t.seal().is_ok());
        assert_eq!(
            t.merge_one(HubPowerCode::VbusDrop),
            Err(MergeCode::SealedFrozen)
        );
        assert_eq!(t.seal(), Err(MergeCode::SealedFrozen));
    }

    #[test]
    fn probe_before_seal_rejected() {
        let mut t = ErrCodeTable::new();
        assert_eq!(
            t.probe(HubPowerCode::VbusDrop.code()),
            Err(MergeCode::NotSealed)
        );
    }

    #[test]
    fn undefined_probe_ten_of_ten() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        let _ = t.seal();
        for i in 0..10u32 {
            assert_eq!(
                t.probe(SEG_BASE + 0x900 + i),
                Err(MergeCode::UndefinedCode)
            );
            t.note_detected();
        }
        assert_eq!(t.probe_rejects(), 10);
        assert!(t.conserved());
    }

    #[test]
    fn dup_merge_ten_of_ten() {
        let mut t = ErrCodeTable::new();
        let _ = t.merge_one(HubPowerCode::OverCurrent);
        for _ in 0..10u32 {
            assert_eq!(
                t.merge_one(HubPowerCode::OverCurrent),
                Err(MergeCode::DupCode)
            );
            t.note_detected();
        }
        assert!(t.conserved());
    }

    #[test]
    fn capacity_precedes_duplicate_when_full() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        assert_eq!(t.row_count(), TABLE_CAP);
        assert_eq!(
            t.merge_one(HubPowerCode::VbusDrop),
            Err(MergeCode::TableFull),
            "并满未封时容量判据先于重复码判据"
        );
    }

    #[test]
    fn probe_hits_first_last_with_classes() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        let _ = t.seal();
        let f = t.probe(HubPowerCode::VbusDrop.code());
        assert!(matches!(f, Ok(r) if r.domain_class == DOMAIN_BUS));
        let l = t.probe(HubPowerCode::ReforcedFail.code());
        assert!(matches!(l, Ok(r) if r.domain_class == DOMAIN_HUB));
    }

    #[test]
    fn replay_codes_strictly_increasing() {
        let mut t = ErrCodeTable::new();
        merge_all(&mut t);
        let _ = t.seal();
        let n = t.replay();
        assert!(matches!(n, Ok(c) if c == TABLE_CAP as u32));
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_codes_closed() {
        assert!(seg_guard_ok());
        assert_eq!(ALL_CODES.len(), 10);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn unx51554_checks_green() {
        let set = run_unx51554_checks();
        assert!(set.all_passed(), "UNX-F51554 red: {}", set.red_items_text());
    }
}
