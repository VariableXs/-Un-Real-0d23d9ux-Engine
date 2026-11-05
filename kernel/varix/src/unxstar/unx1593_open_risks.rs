//! UNX-F1593 域级风险台账收官清账（open_risks 逐项归置） · 0x79240段
//!
//! 判据原文（UNX-F1593-J1 摘录）：「风险清账通过（域 open_risks 逐项归置：
//! 关闭/移交/随闸门三态逐项落账），悬空风险 10/10 告警，清账报告归档」；
//! 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 三态逐项落账：关闭（带清账指纹）/移交（带承接方）/随闸门（带闸门引用）
//!    三种归置态闭集，逐项落账、逐项可查，台账内不得存在无归置项；
//! 2. 悬空风险 10/10 告警：十笔悬空探测逐笔入账不入置，清账口逐笔点名——
//!    悬空不点名即告警面失守，十笔必须全数被点出；
//! 3. 清账报告归档：逐险一行（险名/态/载荷）+ 汇总三态计数，归档前报告
//!    非空且行数与险数对齐，空报告显性拒归档；
//! 4. 相位机 建账中→清账中→已归档 单向，倒退显性拒；已归档后写入全拒；
//! 5. 诊断码闭集 0x79241..0x7924C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79240;

/// 台账险数（收官清账钦定十笔）。
pub const RISK_COUNT: usize = 10;

/// 悬空探测种数（判据原文 10/10）。
pub const HANGING_PROBES: usize = 10;

/// 随闸门可引用的闸门名册（收官批闸门）。
pub const GATE_NAMES: [&str; 3] = ["收官闸门G1", "收官闸门G2", "收官闸门G3"];

/// 清账指纹折叠种子（FNV-1a 偏移基）。
pub const DIGEST_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 清账指纹折叠质数（FNV-1a 质数）。
pub const DIGEST_PRIME: u64 = 0x100_0000_01B3;

/// 风险台账收官清账诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 相位倒退（建账中→清账中→已归档 单向）。
    EPhase,
    /// 重复立账（险号已用）。
    EDupId,
    /// 空标题拒收。
    EEmptyTitle,
    /// 未立账先归置（险号不在台账）。
    ENoEntry,
    /// 未入清账相位先归置。
    EOrder,
    /// 归置载荷非法（关闭无指纹/移交无承接方）。
    EDispBad,
    /// 闸门引用非法（随闸门但闸门不在名册）。
    EGate,
    /// 重复归置（已有归置态再归置）。
    EDupDisp,
    /// 悬空风险未清即归档。
    EHanging,
    /// 已归档后写入拒。
    EArchived,
    /// 清账报告缺失（空报告不归档）。
    ENoReport,
    /// 清账未发起即查归置明细（相位先决未过）。
    ENotClearing,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EPhase => 0x01,
                Diag::EDupId => 0x02,
                Diag::EEmptyTitle => 0x03,
                Diag::ENoEntry => 0x04,
                Diag::EOrder => 0x05,
                Diag::EDispBad => 0x06,
                Diag::EGate => 0x07,
                Diag::EDupDisp => 0x08,
                Diag::EHanging => 0x09,
                Diag::EArchived => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::ENotClearing => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EPhase => "相位倒退：建账中→清账中→已归档 单向，倒退即拒",
            Diag::EDupId => "重复立账：险号已用，先查台账再用号，勿覆盖旧险",
            Diag::EEmptyTitle => "空标题拒收：无题不成险，先补险名再立账",
            Diag::ENoEntry => "未立账先归置：险号不在台账，先立账再归置",
            Diag::EOrder => "未入清账相位先归置：先转清账中再逐项落账",
            Diag::EDispBad => "归置载荷非法：关闭须带清账指纹、移交须带承接方，补载荷再落账",
            Diag::EGate => "闸门引用非法：随闸门只收官收批闸门，按名册引用",
            Diag::EDupDisp => "重复归置：该项已有归置态，改动走台账修订面",
            Diag::EHanging => "悬空风险未清：清账前逐项落三态，悬空零容忍",
            Diag::EArchived => "已归档后写入拒：台账已封，改动走下一轮风险账",
            Diag::ENoReport => "清账报告缺失：先出逐险报告再归档，空报告不收",
            Diag::ENotClearing => "清账未发起：先转清账相位，再查归置明细",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EPhase,
    Diag::EDupId,
    Diag::EEmptyTitle,
    Diag::ENoEntry,
    Diag::EOrder,
    Diag::EDispBad,
    Diag::EGate,
    Diag::EDupDisp,
    Diag::EHanging,
    Diag::EArchived,
    Diag::ENoReport,
    Diag::ENotClearing,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79241..0x7924C 闭集内"
}

/// 险级三档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    /// 高险（清账前必须逐项点名）。
    High,
    /// 中险。
    Mid,
    /// 低险。
    Low,
}

impl Sev {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Sev::High => "高",
            Sev::Mid => "中",
            Sev::Low => "低",
        }
    }
}

/// 险级闭集数组。
pub const SEV_ALL: &[Sev] = &[Sev::High, Sev::Mid, Sev::Low];

/// 归置三态闭集（判据原文：关闭/移交/随闸门）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 关闭（带清账指纹）。
    Closed,
    /// 移交（带承接方）。
    HandedOver,
    /// 随闸门（带闸门引用）。
    GateBound,
}

impl Disposition {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Disposition::Closed => "关闭",
            Disposition::HandedOver => "移交",
            Disposition::GateBound => "随闸门",
        }
    }
}

/// 归置三态闭集数组。
pub const DISP_ALL: &[Disposition] = &[
    Disposition::Closed,
    Disposition::HandedOver,
    Disposition::GateBound,
];

/// 台账相位（建账中→清账中→已归档 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerPhase {
    /// 建账中：逐险登记。
    Registering,
    /// 清账中：逐项归置落账。
    Clearing,
    /// 已归档：清账报告落档封账。
    Archived,
}

impl LedgerPhase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            LedgerPhase::Registering => 0,
            LedgerPhase::Clearing => 1,
            LedgerPhase::Archived => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            LedgerPhase::Registering => "建账中",
            LedgerPhase::Clearing => "清账中",
            LedgerPhase::Archived => "已归档",
        }
    }
}

/// 一笔风险（险号 + 险名 + 险级 + 归置态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskEntry {
    /// 险号（台账内唯一）。
    pub id: u32,
    /// 险名（非空）。
    pub title: &'static str,
    /// 险级。
    pub sev: Sev,
    /// 归置态（None=悬空）。
    pub disposition: Option<Disposition>,
}

/// 归置载荷（随态取用：关闭取指纹、移交取承接方、随闸门取闸门引用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispPayload {
    /// 关闭态清账指纹（其余态忽略，置零）。
    pub close_digest: u64,
    /// 移交态承接方（其余态空串）。
    pub consumer: &'static str,
    /// 随闸门态闸门引用（其余态空串）。
    pub gate: &'static str,
}

/// 风险台账（收官清账面）。
#[derive(Clone, Debug)]
pub struct RiskLedger {
    phase: LedgerPhase,
    entries: Vec<RiskEntry>,
    /// 结构性拒绝计数。
    rejects: u32,
    /// 悬空告警计数（清账口点名次数）。
    hang_alerts: u32,
}

impl RiskLedger {
    /// 新台账（建账中）。
    pub fn new() -> RiskLedger {
        RiskLedger {
            phase: LedgerPhase::Registering,
            entries: Vec::new(),
            rejects: 0,
            hang_alerts: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> LedgerPhase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 悬空告警计数。
    pub fn hang_alerts(&self) -> u32 {
        self.hang_alerts
    }

    /// 已立险数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 立账（险号唯一、险名非空；建账中/清账中皆可补立）。
    pub fn register(&mut self, id: u32, title: &'static str, sev: Sev) -> Result<(), u32> {
        if self.phase == LedgerPhase::Archived {
            self.rejects += 1;
            return Err(Diag::EArchived.val());
        }
        if title.is_empty() {
            self.rejects += 1;
            return Err(Diag::EEmptyTitle.val());
        }
        if self.entries.iter().any(|e| e.id == id) {
            self.rejects += 1;
            return Err(Diag::EDupId.val());
        }
        self.entries.push(RiskEntry {
            id,
            title,
            sev,
            disposition: None,
        });
        Ok(())
    }

    /// 转清账中（单向，倒退拒）。
    pub fn begin_clearing(&mut self) -> Result<(), u32> {
        if self.phase != LedgerPhase::Registering {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if self.entries.is_empty() {
            return Err(Diag::ENoEntry.val());
        }
        self.phase = LedgerPhase::Clearing;
        Ok(())
    }

    /// 按险号取项。
    pub fn entry_of(&self, id: u32) -> Option<&RiskEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 归置一项（清账中限定；载荷随态校验）。
    pub fn dispose(&mut self, id: u32, disp: Disposition, payload: &DispPayload) -> Result<(), u32> {
        if self.phase == LedgerPhase::Archived {
            self.rejects += 1;
            return Err(Diag::EArchived.val());
        }
        if self.phase != LedgerPhase::Clearing {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        let slot = match self.entries.iter_mut().find(|e| e.id == id) {
            Some(e) => e,
            None => {
                self.rejects += 1;
                return Err(Diag::ENoEntry.val());
            }
        };
        if slot.disposition.is_some() {
            self.rejects += 1;
            return Err(Diag::EDupDisp.val());
        }
        match disp {
            Disposition::Closed => {
                if payload.close_digest == 0 {
                    self.rejects += 1;
                    return Err(Diag::EDispBad.val());
                }
            }
            Disposition::HandedOver => {
                if payload.consumer.is_empty() {
                    self.rejects += 1;
                    return Err(Diag::EDispBad.val());
                }
            }
            Disposition::GateBound => {
                if payload.gate.is_empty() || !GATE_NAMES.iter().any(|g| *g == payload.gate) {
                    self.rejects += 1;
                    return Err(Diag::EGate.val());
                }
            }
        }
        slot.disposition = Some(disp);
        Ok(())
    }

    /// 悬空清单（无归置项险号，按立账序）。
    pub fn hanging(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|e| e.disposition.is_none())
            .map(|e| e.id)
            .collect()
    }

    /// 悬空点名告警：清账口逐笔点出（返回本次点名笔数）。
    pub fn alert_hanging(&mut self) -> u32 {
        let n = self.hanging().len() as u32;
        self.hang_alerts += n;
        n
    }

    /// 清账报告（逐险一行 + 三态汇总；未入清账相位拒）。
    pub fn report(&self) -> Result<String, u32> {
        if self.phase == LedgerPhase::Registering {
            return Err(Diag::ENotClearing.val());
        }
        let mut out = String::new();
        let mut closed = 0usize;
        let mut handed = 0usize;
        let mut gatebound = 0usize;
        for e in &self.entries {
            let disp_name = match e.disposition {
                Some(d) => {
                    match d {
                        Disposition::Closed => closed += 1,
                        Disposition::HandedOver => handed += 1,
                        Disposition::GateBound => gatebound += 1,
                    }
                    d.name()
                }
                None => "悬空",
            };
            out.push_str(&format!(
                "险{} {} {}级 {}\n",
                e.id, e.title, e.sev.name(), disp_name
            ));
        }
        out.push_str(&format!(
            "汇总 关闭{} 移交{} 随闸门{} 悬空{}\n",
            closed,
            handed,
            gatebound,
            self.hanging().len()
        ));
        Ok(out)
    }

    /// 归档（悬空零容忍 + 报告非空才封账；单向，倒退拒）。
    pub fn archive(&mut self) -> Result<String, u32> {
        if self.phase != LedgerPhase::Clearing {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let hang = self.hanging();
        if !hang.is_empty() {
            let _ = self.alert_hanging();
            return Err(Diag::EHanging.val());
        }
        let report = self.report()?;
        if report.is_empty() {
            return Err(Diag::ENoReport.val());
        }
        self.phase = LedgerPhase::Archived;
        Ok(report)
    }
}

/// 清账指纹（FNV-1a 整数折叠）。
pub fn closure_digest(title: &str) -> u64 {
    let mut h = DIGEST_SEED;
    for b in title.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(DIGEST_PRIME);
    }
    h.wrapping_mul(DIGEST_PRIME)
}

/// 十险名册（收官清账钦定，编号 1..=10）。
pub const RISK_TITLES: [&str; RISK_COUNT] = [
    "口径漂移险",
    "移交断点险",
    "资产重复险",
    "观测盲区险",
    "签名过期险",
    "文档失同步险",
    "回归漏项险",
    "负载溢出险",
    "闸门误跳险",
    "沉淀失溯源险",
];

/// 十险归置态（钦定混布：四关三移三闸，覆盖三态全谱）。
pub const RISK_DISPS: [Disposition; RISK_COUNT] = [
    Disposition::Closed,
    Disposition::HandedOver,
    Disposition::GateBound,
    Disposition::Closed,
    Disposition::GateBound,
    Disposition::HandedOver,
    Disposition::Closed,
    Disposition::HandedOver,
    Disposition::GateBound,
    Disposition::Closed,
];

/// 标准台账：十险齐立、逐项按钦定态落账、已入清账相位（未归档）。
pub fn build_ledger() -> RiskLedger {
    let mut ledger = RiskLedger::new();
    let mut i = 0usize;
    while i < RISK_COUNT {
        let title = match RISK_TITLES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let sev = if i % 3 == 0 {
            Sev::High
        } else if i % 3 == 1 {
            Sev::Mid
        } else {
            Sev::Low
        };
        let _ = ledger.register((i + 1) as u32, title, sev);
        i += 1;
    }
    let _ = ledger.begin_clearing();
    let mut i = 0usize;
    while i < RISK_COUNT {
        let disp = match RISK_DISPS.get(i) {
            Some(x) => *x,
            None => break,
        };
        let title = match RISK_TITLES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let payload = DispPayload {
            close_digest: closure_digest(title),
            consumer: "消费域承接组",
            gate: match GATE_NAMES.get(i % GATE_NAMES.len()) {
                Some(g) => *g,
                None => "",
            },
        };
        let _ = ledger.dispose((i + 1) as u32, disp, &payload);
        i += 1;
    }
    ledger
}

/// 悬空探测 10/10：十笔探测逐笔入账不入置，逐笔必须被悬空清单点出。
/// 返回十布尔（true=该笔被点名）。
pub fn dangling_matrix() -> [bool; HANGING_PROBES] {
    let mut ledger = RiskLedger::new();
    let mut i = 0usize;
    while i < HANGING_PROBES {
        let title = match RISK_TITLES.get(i) {
            Some(x) => *x,
            None => break,
        };
        let _ = ledger.register((i + 1) as u32, title, Sev::High);
        i += 1;
    }
    let hang = ledger.hanging();
    let mut out = [false; HANGING_PROBES];
    let mut i = 0usize;
    while i < HANGING_PROBES {
        let id = (i + 1) as u32;
        let flagged = hang.iter().any(|x| *x == id);
        if let Some(cell) = out.get_mut(i) {
            *cell = flagged;
        }
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79240
}

/// 三态闭集（不重不漏）。
fn disp_closed_ok() -> bool {
    if DISP_ALL.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < DISP_ALL.len() {
        let a = match DISP_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < DISP_ALL.len() {
            let b = match DISP_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 清账通过（十险齐、逐项落账、悬空为零）。
fn cleared_ok(ledger: &RiskLedger) -> bool {
    if ledger.len() != RISK_COUNT || !ledger.hanging().is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < RISK_COUNT {
        let e = match ledger.entry_of((i + 1) as u32) {
            Some(e) => e,
            None => return false,
        };
        if e.disposition.is_none() {
            return false;
        }
        i += 1;
    }
    true
}

/// 三态混布齐谱（标准台账三态各至少一笔）。
fn mixed_disps_ok(ledger: &RiskLedger) -> bool {
    let report = match ledger.report() {
        Ok(t) => t,
        Err(_) => return false,
    };
    report.contains("关闭") && report.contains("移交") && report.contains("随闸门")
}

/// 悬空 10/10 告警。
fn hanging_all_ok() -> bool {
    dangling_matrix() == [true; HANGING_PROBES]
}

/// 双跑一致（同构输入两次构建台账清账同判）。
fn dual_run_ok() -> bool {
    let a = build_ledger();
    let b = build_ledger();
    match (a.report(), b.report()) {
        (Ok(x), Ok(y)) => x == y && !x.is_empty(),
        _ => false,
    }
}

/// 风险清账判据：UNX-F1593 域级风险台账收官清账。
pub fn run_unx1593_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1593");
    let mut ledger = build_ledger();

    s.add_dyn(
        "F1593-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1593-02 归置三态闭集",
        disp_closed_ok(),
        &format!("states={}/{}/{}",
            DISP_ALL.get(0).map(|d| d.name()).unwrap_or(""),
            DISP_ALL.get(1).map(|d| d.name()).unwrap_or(""),
            DISP_ALL.get(2).map(|d| d.name()).unwrap_or("")),
    );

    // 正常流：十险齐立逐项落账，悬空为零。
    s.add_dyn(
        "F1593-03 十险齐立逐项落账",
        cleared_ok(&ledger),
        &format!("entries={} hanging={}", ledger.len(), ledger.hanging().len()),
    );

    // 三态混布齐谱（报告含三态名）。
    s.add_dyn(
        "F1593-04 三态混布齐谱",
        mixed_disps_ok(&ledger),
        &format!("phase={}", ledger.phase().name()),
    );

    // 结构拒绝：空题、重号、未立账先归置。
    let mut r = RiskLedger::new();
    let empty_title = r.register(1, "", Sev::High) == Err(Diag::EEmptyTitle.val());
    let dup = r.register(2, "试险甲", Sev::High) == Ok(())
        && r.register(2, "试险乙", Sev::Mid) == Err(Diag::EDupId.val());
    let _ = r.begin_clearing();
    let no_entry = r.dispose(99, Disposition::Closed,
        &DispPayload { close_digest: 1, consumer: "", gate: "" }) == Err(Diag::ENoEntry.val());
    s.add_dyn(
        "F1593-05 立账归置结构三拒",
        empty_title && dup && no_entry,
        &format!("rejects={}", r.rejects()),
    );

    // 相位先决：建账中归置显性拒。
    let mut early = RiskLedger::new();
    let _ = early.register(1, "早险", Sev::Low);
    let early_rej = early.dispose(1, Disposition::Closed,
        &DispPayload { close_digest: 1, consumer: "", gate: "" }) == Err(Diag::EOrder.val());
    s.add_dyn(
        "F1593-06 建账中归置显性拒",
        early_rej && early.phase() == LedgerPhase::Registering,
        &format!("phase={}", early.phase().name()),
    );

    // 载荷校验：关闭零指纹/移交空承接方/闸门册外引用逐门拒
    //（未归置项上逐面试，三面皆拒且不落账）。
    let mut p = RiskLedger::new();
    let _ = p.register(1, "试险", Sev::High);
    let _ = p.begin_clearing();
    let bad_digest = p.dispose(1, Disposition::Closed,
        &DispPayload { close_digest: 0, consumer: "", gate: "" }) == Err(Diag::EDispBad.val());
    let bad_consumer = p.dispose(1, Disposition::HandedOver,
        &DispPayload { close_digest: 0, consumer: "", gate: "" }) == Err(Diag::EDispBad.val());
    let bad_gate = p.dispose(1, Disposition::GateBound,
        &DispPayload { close_digest: 0, consumer: "", gate: "册外闸门" }) == Err(Diag::EGate.val());
    s.add_dyn(
        "F1593-07 归置载荷三面逐门拒",
        bad_digest && bad_consumer && bad_gate,
        &format!("rejects={}", p.rejects()),
    );

    // 重复归置显性拒（已归置项上重归置）。
    let mut q = build_ledger();
    let dup_disp = q.dispose(1, Disposition::Closed,
        &DispPayload { close_digest: 7, consumer: "", gate: "" }) == Err(Diag::EDupDisp.val());
    s.add_dyn(
        "F1593-08 重复归置显性拒",
        dup_disp,
        &format!("dup_rejected={}", dup_disp),
    );

    // 悬空风险 10/10 告警。
    s.add_dyn(
        "F1593-09 悬空风险十笔全点名",
        hanging_all_ok(),
        &format!("matrix={:?}", dangling_matrix()),
    );

    // 悬空未清归档显性拒且计数入账。
    let mut hang_ledger = RiskLedger::new();
    let _ = hang_ledger.register(1, "悬险", Sev::High);
    let _ = hang_ledger.begin_clearing();
    let hang_rej = hang_ledger.archive() == Err(Diag::EHanging.val());
    s.add_dyn(
        "F1593-10 悬空未清归档显性拒",
        hang_rej && hang_ledger.hang_alerts() == 1,
        &format!("alerts={}", hang_ledger.hang_alerts()),
    );

    // 清账报告归档：归档返回非空报告、相位转已归档、再写全拒。
    let archived = ledger.archive();
    let report_ok = match &archived {
        Ok(t) => !t.is_empty() && t.contains("汇总") && t.matches('\n').count() == RISK_COUNT + 1,
        Err(_) => false,
    };
    s.add_dyn(
        "F1593-11 清账报告归档非空",
        report_ok && ledger.phase() == LedgerPhase::Archived,
        &format!("bytes={}", archived.as_ref().map(|t| t.len()).unwrap_or(0)),
    );

    // 已归档后写入显性拒（立账/归置/再归档三口全拒）。
    let after_reg = ledger.register(11, "迟险", Sev::Low) == Err(Diag::EArchived.val());
    let after_disp = ledger.dispose(1, Disposition::Closed,
        &DispPayload { close_digest: 1, consumer: "", gate: "" }) == Err(Diag::EArchived.val());
    let after_arch = ledger.archive() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1593-12 已归档后三口全拒",
        after_reg && after_disp && after_arch && ledger.phase().ord() == 2,
        &format!("phase_ord={}", ledger.phase().ord()),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1593-13 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::ENotClearing.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ENotClearing.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79240);
    }

    #[test]
    fn disp_closed_set() {
        assert_eq!(DISP_ALL.len(), 3);
        assert_ne!(Disposition::Closed, Disposition::HandedOver);
        assert_ne!(Disposition::HandedOver, Disposition::GateBound);
        assert_ne!(Disposition::GateBound, Disposition::Closed);
    }

    #[test]
    fn register_gates() {
        let mut r = RiskLedger::new();
        assert_eq!(r.register(1, "", Sev::High), Err(Diag::EEmptyTitle.val()));
        assert_eq!(r.register(1, "甲险", Sev::High), Ok(()));
        assert_eq!(r.register(1, "乙险", Sev::Mid), Err(Diag::EDupId.val()));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn dispose_requires_clearing_phase() {
        let mut r = RiskLedger::new();
        let _ = r.register(1, "甲险", Sev::High);
        let payload = DispPayload { close_digest: 1, consumer: "", gate: "" };
        assert_eq!(r.dispose(1, Disposition::Closed, &payload), Err(Diag::EOrder.val()));
        assert_eq!(r.begin_clearing(), Ok(()));
        assert_eq!(r.dispose(1, Disposition::Closed, &payload), Ok(()));
    }

    #[test]
    fn payload_gates() {
        // 未归置项上逐面试载荷（三面皆拒且不落账）。
        let mut l = RiskLedger::new();
        let _ = l.register(1, "甲险", Sev::High);
        let _ = l.begin_clearing();
        assert_eq!(
            l.dispose(1, Disposition::Closed, &DispPayload { close_digest: 0, consumer: "", gate: "" }),
            Err(Diag::EDispBad.val())
        );
        assert_eq!(
            l.dispose(1, Disposition::HandedOver, &DispPayload { close_digest: 0, consumer: "", gate: "" }),
            Err(Diag::EDispBad.val())
        );
        assert_eq!(
            l.dispose(1, Disposition::GateBound, &DispPayload { close_digest: 0, consumer: "", gate: "册外" }),
            Err(Diag::EGate.val())
        );
        assert_eq!(l.hanging(), vec![1u32], "三面皆拒后仍未归置");
        // 已归置项重归置显性拒。
        let mut q = build_ledger();
        assert_eq!(
            q.dispose(1, Disposition::Closed, &DispPayload { close_digest: 9, consumer: "", gate: "" }),
            Err(Diag::EDupDisp.val())
        );
    }

    #[test]
    fn dangling_ten_of_ten() {
        assert_eq!(dangling_matrix(), [true; HANGING_PROBES]);
        let mut l = RiskLedger::new();
        let _ = l.register(1, "悬险", Sev::High);
        assert_eq!(l.hanging(), vec![1u32]);
    }

    #[test]
    fn hanging_blocks_archive() {
        let mut l = RiskLedger::new();
        let _ = l.register(1, "悬险", Sev::High);
        let _ = l.register(2, "清险", Sev::Low);
        let _ = l.begin_clearing();
        let _ = l.dispose(2, Disposition::Closed, &DispPayload { close_digest: 5, consumer: "", gate: "" });
        assert_eq!(l.archive(), Err(Diag::EHanging.val()));
        assert_eq!(l.hang_alerts(), 1, "悬空点名必须计数入账");
        assert_eq!(l.phase(), LedgerPhase::Clearing);
    }

    #[test]
    fn baseline_cleared_and_mixed() {
        let l = build_ledger();
        assert_eq!(l.len(), RISK_COUNT);
        assert!(l.hanging().is_empty());
        let report = l.report().unwrap_or_default();
        assert!(report.contains("关闭"));
        assert!(report.contains("移交"));
        assert!(report.contains("随闸门"));
        assert!(report.contains("悬空0"));
    }

    #[test]
    fn archive_seals_and_rejects_writes() {
        let mut l = build_ledger();
        let archived = l.archive();
        assert!(archived.is_ok());
        assert_eq!(l.phase(), LedgerPhase::Archived);
        assert_eq!(l.register(11, "迟险", Sev::Low), Err(Diag::EArchived.val()));
        assert_eq!(
            l.dispose(1, Disposition::Closed, &DispPayload { close_digest: 1, consumer: "", gate: "" }),
            Err(Diag::EArchived.val())
        );
        assert_eq!(l.archive(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn report_needs_clearing_phase() {
        let mut l = RiskLedger::new();
        let _ = l.register(1, "甲险", Sev::High);
        assert_eq!(l.report(), Err(Diag::ENotClearing.val()));
        let _ = l.begin_clearing();
        assert!(l.report().is_ok());
    }

    #[test]
    fn closure_digest_stable_and_nonzero() {
        let a = closure_digest("口径漂移险");
        let b = closure_digest("口径漂移险");
        let c = closure_digest("移交断点险");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn phase_monotonic() {
        let mut l = RiskLedger::new();
        assert_eq!(l.phase(), LedgerPhase::Registering);
        // 建账中直接归档显性拒（相位先决）。
        assert_eq!(l.archive(), Err(Diag::EPhase.val()));
        let _ = l.register(1, "甲险", Sev::High);
        let _ = l.begin_clearing();
        assert_eq!(l.begin_clearing(), Err(Diag::EPhase.val()), "清账中不可重入清账");
        let _ = l.dispose(1, Disposition::Closed, &DispPayload { close_digest: 3, consumer: "", gate: "" });
        assert_eq!(l.archive().is_ok(), true);
        assert_eq!(l.phase(), LedgerPhase::Archived);
        assert_eq!(l.phase().ord(), 2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1593_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
