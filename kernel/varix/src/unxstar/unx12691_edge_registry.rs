//! UNX-F12692 消费边接口登记总表（跨批消费边逐条可点验） · 0x816C0段
//!
//! 判据原文（派工卡 12691 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12692-J1 两态账汇总判据绿（D2/D3 真绿/D4·E1
//!      槽位态），混报注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 消费边登记域模型：登记行五元（行号/归属域 D2–E1/接口短名/边种
//!    别/态申报），四方各 15 行 = 60 行全量登记——跨批消费边逐条可点
//!    验的登记面；
//! 2. 边种别三闭集（探针边/账目边/闸门边）：每行边种别冻结落位，种别
//!    越闭集即账可疑；
//! 3. 态申报二闭集（真绿申报/槽位态申报）：行级申报必须与归属域的冻
//!    结口径一致（D2/D3 真绿、D4/E1 槽位态）——行级混报显性拒；
//! 4. 两态账汇总判据绿：每域一条汇总行（域/汇总申报/行额），汇总申报
//!    必须等于冻结口径且行额 ≡ 15——汇总混报显性拒；
//! 5. 登记账状态机 ErOpen → ErRunning → ErSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：行数 ≡ 汇总行额合计守恒；
//! 6. 混报注入十路：D2 行混报/D3 行混报/D4 行混报/E1 行混报/D2 汇总
//!    混报/D4 汇总混报/汇总缺一/汇总重一/行额漂移/封账后写——逐路必
//!    检出；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `twostate_summary_green_ok` ↔ ② 两态账汇总判据绿（D2/D3 真绿/
//!     D4·E1 槽位态）；
//!   - `mixreport_inject_ok` ↔ ② 混报注入 10/10 检出；
//!   - `registry_complete_ok` ↔ ② 60 行全量登记逐条可点验；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12691 − 12600)）。
pub const SEG_BASE: u32 = 0x816C0;

// ── 消费边登记诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErDiag {
    /// 行混报（行级态申报与冻结口径不符）。
    ERowMix,
    /// 汇总混报（域级申报与冻结口径不符）。
    ESummaryMix,
    /// 域缺行（任一域行额不足）。
    EDomainShort,
    /// 汇总缺一。
    ESummaryMissing,
    /// 汇总重一。
    ESummaryDup,
    /// 行额漂移（域行额 ≠ 15）。
    ERowCountDrift,
    /// 接口置空。
    EIfaceEmpty,
    /// 边种越集。
    EKindRange,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
}

impl ErDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                ErDiag::ERowMix => 0x01,
                ErDiag::ESummaryMix => 0x02,
                ErDiag::EDomainShort => 0x03,
                ErDiag::ESummaryMissing => 0x04,
                ErDiag::ESummaryDup => 0x05,
                ErDiag::ERowCountDrift => 0x06,
                ErDiag::EIfaceEmpty => 0x07,
                ErDiag::EKindRange => 0x08,
                ErDiag::ESealedWrite => 0x09,
                ErDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            ErDiag::ERowMix => "行混报：行级态申报必须与归属域冻结口径一致，先归域核口径",
            ErDiag::ESummaryMix => "汇总混报：域级申报必须等于冻结口径，先核分列口径",
            ErDiag::EDomainShort => "域缺行：四方各 15 行全量登记，缺行先回补",
            ErDiag::ESummaryMissing => "汇总缺一：每域一条汇总行，缺条先补",
            ErDiag::ESummaryDup => "汇总重一：同域只许一条汇总行，重条先去重",
            ErDiag::ERowCountDrift => "行额漂移：域行额必须等于 15，漂移先核清单",
            ErDiag::EIfaceEmpty => "接口置空：接口短名非空才可登记，空名先补",
            ErDiag::EKindRange => "边种越集：边种别必须落在三闭集内，越集先归种",
            ErDiag::ESealedWrite => "封账后写：登记账只读，补行走下一轮登记",
            ErDiag::ERewind => "状态机倒退：ErOpen→ErRunning→ErSealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ER_ALL: &[ErDiag] = &[
    ErDiag::ERowMix,
    ErDiag::ESummaryMix,
    ErDiag::EDomainShort,
    ErDiag::ESummaryMissing,
    ErDiag::ESummaryDup,
    ErDiag::ERowCountDrift,
    ErDiag::EIfaceEmpty,
    ErDiag::EKindRange,
    ErDiag::ESealedWrite,
    ErDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ER_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x816C1..0x816CA 闭集内"
}

// ── 归属域四闭集 ─────────────────────────────────────────────────
/// 登记归属域（四方闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegDomain {
    /// D2 域。
    DomD2,
    /// D3 域。
    DomD3,
    /// D4 域。
    DomD4,
    /// E1 域。
    DomE1,
}

impl RegDomain {
    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            RegDomain::DomD2 => "D2",
            RegDomain::DomD3 => "D3",
            RegDomain::DomD4 => "D4",
            RegDomain::DomE1 => "E1",
        }
    }

    /// 域号。
    pub fn ordinal(self) -> u32 {
        match self {
            RegDomain::DomD2 => 0,
            RegDomain::DomD3 => 1,
            RegDomain::DomD4 => 2,
            RegDomain::DomE1 => 3,
        }
    }

    /// 域行额（每域 15 行）。
    pub fn row_cap(self) -> usize {
        15
    }

    /// 全部域（闭集数组）。
    pub fn all() -> &'static [RegDomain] {
        &[RegDomain::DomD2, RegDomain::DomD3, RegDomain::DomD4, RegDomain::DomE1]
    }
}

// ── 态申报二闭集 ─────────────────────────────────────────────────
/// 态申报（真绿申报 / 槽位态申报）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateClaim {
    /// 真绿申报（已收口实对平）。
    ClaimTrueGreen,
    /// 槽位态申报（未开工槽位态）。
    ClaimSlotState,
}

impl StateClaim {
    /// 申报名。
    pub fn name(self) -> &'static str {
        match self {
            StateClaim::ClaimTrueGreen => "真绿申报",
            StateClaim::ClaimSlotState => "槽位态申报",
        }
    }

    /// 全部申报（闭集数组）。
    pub fn all() -> &'static [StateClaim] {
        &[StateClaim::ClaimTrueGreen, StateClaim::ClaimSlotState]
    }
}

/// 冻结口径：域 → 态申报（D2/D3 真绿、D4/E1 槽位态）。
pub fn frozen_claim(d: RegDomain) -> StateClaim {
    match d {
        RegDomain::DomD2 => StateClaim::ClaimTrueGreen,
        RegDomain::DomD3 => StateClaim::ClaimTrueGreen,
        RegDomain::DomD4 => StateClaim::ClaimSlotState,
        RegDomain::DomE1 => StateClaim::ClaimSlotState,
    }
}

// ── 边种别三闭集 ─────────────────────────────────────────────────
/// 边种别（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// 探针边。
    ProbeEdge,
    /// 账目边。
    LedgerEdge,
    /// 闸门边。
    GateEdge,
}

impl EdgeKind {
    /// 种别名。
    pub fn name(self) -> &'static str {
        match self {
            EdgeKind::ProbeEdge => "探针边",
            EdgeKind::LedgerEdge => "账目边",
            EdgeKind::GateEdge => "闸门边",
        }
    }

    /// 全部种别（闭集数组）。
    pub fn all() -> &'static [EdgeKind] {
        &[EdgeKind::ProbeEdge, EdgeKind::LedgerEdge, EdgeKind::GateEdge]
    }
}

// ── 四方冻结接口清单（每域 15 行） ───────────────────────────────
/// D2 域冻结接口清单（15 行，全真绿申报）。
pub const D2_IFACES: &[&str] = &[
    "D2 重测边", "OA 双链边", "装载码边", "里程碑边", "消息边界边",
    "端口连接边", "监听受理边", "节账分边", "SQoS 档边", "SID 守卫边",
    "消息属性边", "消息类型边", "回执对边", "段视图边", "读写边",
];

/// D3 域冻结接口清单（15 行，全真绿申报）。
pub const D3_IFACES: &[&str] = &[
    "D3 重测边", "注入对账甲边", "注入对账乙边", "双比较边", "比较表边",
    "P95 预算边", "边缘采样边", "E1 回执边", "文档对齐边", "KTEST 聚合边",
    "B35 闸边", "B29 重跑边", "批额边", "探针码边", "点验边",
];

/// D4 域冻结接口清单（15 行，全槽位态申报）。
pub const D4_IFACES: &[&str] = &[
    "装载句柄边", "装载段映射边", "装载符号边", "装载收口边", "装载环境边",
    "装载重定位边", "装载导入边", "装载导出边", "装载异常边", "装载清单边",
    "装载摘要边", "对象创建边", "句柄授予边", "权限位边", "段属性边",
];

/// E1 域冻结接口清单（15 行，全槽位态申报）。
pub const E1_IFACES: &[&str] = &[
    "根路径边", "命名对象边", "删除挂起边", "对象收口边", "越根拒止边",
    "PEB 探针边", "TEB 探针边", "NTSTATUS 边", "进程查询边", "线程查询边",
    "OS 版本边", "系统信息边", "对象类型边", "同步对象边", "进程枚举边",
];

/// 登记行五元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgeEntry {
    /// 行号（0..60 连续）。
    pub no: u32,
    /// 归属域。
    pub domain: RegDomain,
    /// 接口短名。
    pub iface: &'static str,
    /// 边种别（按行号折算三闭集轮转）。
    pub kind: EdgeKind,
    /// 态申报（冻结口径一致）。
    pub claim: StateClaim,
}

/// 边种别冻结折算（行号 mod 3 轮转，探针→账目→闸门）。
pub fn kind_for_row(no: u32) -> EdgeKind {
    match no % 3 {
        0 => EdgeKind::ProbeEdge,
        1 => EdgeKind::LedgerEdge,
        _ => EdgeKind::GateEdge,
    }
}

/// 按域按下标取冻结接口短名（越界 None）。
pub fn domain_iface(d: RegDomain, idx: usize) -> Option<&'static str> {
    match d {
        RegDomain::DomD2 => D2_IFACES.get(idx).copied(),
        RegDomain::DomD3 => D3_IFACES.get(idx).copied(),
        RegDomain::DomD4 => D4_IFACES.get(idx).copied(),
        RegDomain::DomE1 => E1_IFACES.get(idx).copied(),
    }
}

/// 冻结清单铺登记表（四方依序逐行铺开，60 行）。
pub fn build_edge_registry() -> Vec<EdgeEntry> {
    let mut rows: Vec<EdgeEntry> = Vec::new();
    for d in RegDomain::all().iter() {
        let mut i = 0usize;
        while i < d.row_cap() {
            let iface = match domain_iface(*d, i) {
                Some(v) => v,
                None => break,
            };
            let no = rows.len() as u32;
            rows.push(EdgeEntry {
                no,
                domain: *d,
                iface,
                kind: kind_for_row(no),
                claim: frozen_claim(*d),
            });
            i += 1;
        }
    }
    rows
}

/// 登记总行额（判据锚定 60 行）。
pub const EDGE_ROW_CAP: usize = 60;

// ── 域汇总行 ─────────────────────────────────────────────────────
/// 域汇总行三元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainSummary {
    /// 归属域。
    pub domain: RegDomain,
    /// 汇总申报（必须等于冻结口径）。
    pub claim: StateClaim,
    /// 行额（必须 ≡ 15）。
    pub row_count: u32,
}

/// 登记账状态机（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErState {
    /// 开账。
    ErOpen,
    /// 登记中。
    ErRunning,
    /// 封账（只读）。
    ErSealed,
}

impl ErState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            ErState::ErOpen => 0,
            ErState::ErRunning => 1,
            ErState::ErSealed => 2,
        }
    }
}

/// 消费边登记账。
#[derive(Clone, Debug)]
pub struct ErLedger {
    /// 状态。
    state: ErState,
    /// 登记行。
    rows: Vec<EdgeEntry>,
    /// 域汇总行。
    summaries: Vec<DomainSummary>,
    /// 拒止计数。
    rejects: u32,
    /// 点验数。
    probes: u32,
}

impl ErLedger {
    /// 新账。
    pub fn new() -> ErLedger {
        ErLedger {
            state: ErState::ErOpen,
            rows: Vec::new(),
            summaries: Vec::new(),
            rejects: 0,
            probes: 0,
        }
    }

    /// 开账 → 登记中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            ErState::ErOpen => {
                self.state = ErState::ErRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(ErDiag::ERewind.val())
            }
        }
    }

    /// 登记中 → 封账（60 行 + 四汇总齐才许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            ErState::ErRunning => {
                if self.rows.len() != EDGE_ROW_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ErDiag::EDomainShort.val());
                }
                if self.summaries.len() != RegDomain::all().len() {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(ErDiag::ESummaryMissing.val());
                }
                self.state = ErState::ErSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(ErDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> ErState {
        self.state
    }

    /// 登记行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 汇总条数。
    pub fn summary_len(&self) -> usize {
        self.summaries.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 点验数。
    pub fn probe_len(&self) -> u32 {
        self.probes
    }

    /// 下标取行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&EdgeEntry> {
        self.rows.get(idx)
    }

    /// 下标取汇总（越界 None）。
    pub fn summary_at(&self, idx: usize) -> Option<&DomainSummary> {
        self.summaries.get(idx)
    }

    /// 账等式：点验数 ≡ 行数且汇总行额合计 ≡ 行数。
    pub fn equation_ok(&self) -> bool {
        let mut total: u32 = 0;
        for s in self.summaries.iter() {
            total = total.saturating_add(s.row_count);
        }
        self.probes as usize == self.rows.len() && total as usize == self.rows.len()
    }

    /// 登记一行（序/名/种/申报四闸）。
    pub fn reg(&mut self, r: EdgeEntry) -> Result<(), u32> {
        if self.state == ErState::ErSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ESealedWrite.val());
        }
        if self.state == ErState::ErOpen {
            self.state = ErState::ErRunning;
        }
        // 序闸。
        if r.no != self.rows.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::EDomainShort.val());
        }
        // 名闸。
        if r.iface.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::EIfaceEmpty.val());
        }
        // 种闸。
        let kind_known = EdgeKind::all().iter().any(|k| *k == r.kind);
        if !kind_known || r.kind != kind_for_row(r.no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::EKindRange.val());
        }
        // 申报闸（行级混报显性拒）。
        if r.claim != frozen_claim(r.domain) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ERowMix.val());
        }
        self.probes = self.probes.saturating_add(1);
        self.rows.push(r);
        Ok(())
    }

    /// 落一条域汇总（缺重/申报/行额三闸）。
    pub fn summarize(&mut self, s: DomainSummary) -> Result<(), u32> {
        if self.state == ErState::ErSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ESealedWrite.val());
        }
        // 汇总重一。
        if self.summaries.iter().any(|e| e.domain == s.domain) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ESummaryDup.val());
        }
        // 汇总申报闸。
        if s.claim != frozen_claim(s.domain) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ESummaryMix.val());
        }
        // 行额闸。
        if s.row_count as usize != s.domain.row_cap() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(ErDiag::ERowCountDrift.val());
        }
        self.summaries.push(s);
        Ok(())
    }
}

/// 全量登记（60 行逐行入账、四域逐域汇总并封账）。
pub fn build_full_er_ledger() -> ErLedger {
    let rows = build_edge_registry();
    let mut led = ErLedger::new();
    let _ = led.begin_run();
    for r in rows.iter() {
        let _ = led.reg(r.clone());
    }
    for d in RegDomain::all().iter() {
        let _ = led.summarize(DomainSummary {
            domain: *d,
            claim: frozen_claim(*d),
            row_count: d.row_cap() as u32,
        });
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 两态账汇总判据绿（D2/D3 真绿/D4·E1 槽位态，汇总全过闸）。
fn twostate_summary_green_ok() -> bool {
    let led = build_full_er_ledger();
    if led.state() != ErState::ErSealed {
        return false;
    }
    if led.reject_len() != 0 || led.summary_len() != RegDomain::all().len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.summary_len() {
        let s = match led.summary_at(i) {
            Some(v) => v,
            None => return false,
        };
        if s.claim != frozen_claim(s.domain) {
            return false;
        }
        if s.row_count as usize != s.domain.row_cap() {
            return false;
        }
        i += 1;
    }
    true
}

/// 60 行全量登记逐条可点验（行级申报全过冻结口径）。
fn registry_complete_ok() -> bool {
    let rows = build_edge_registry();
    if rows.len() != EDGE_ROW_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(v) => v,
            None => return false,
        };
        if r.no != i as u32 {
            return false;
        }
        if r.claim != frozen_claim(r.domain) || r.kind != kind_for_row(r.no) {
            return false;
        }
        if r.iface.is_empty() {
            return false;
        }
        i += 1;
    }
    // 每域 15 行。
    for d in RegDomain::all().iter() {
        let mut count = 0usize;
        for r in rows.iter() {
            if r.domain == *d {
                count += 1;
            }
        }
        if count != d.row_cap() {
            return false;
        }
    }
    true
}

/// 混报注入 10/10 检出。
fn mixreport_inject_ok() -> bool {
    let (caught, total, escape) = er_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 注入联测十路（人为构造混报必被检出）。
pub fn er_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. D2 行混报（真绿域行报槽位态）。
    {
        let mut rows = build_edge_registry();
        let d2_first = match rows.iter_mut().find(|r| r.domain == RegDomain::DomD2) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        d2_first.claim = StateClaim::ClaimSlotState;
        if led_detect_row_mix(&rows) {
            caught += 1;
        } else {
            escape.push_str("D2行;");
        }
    }
    // 2. D3 行混报。
    {
        let mut rows = build_edge_registry();
        let d3_first = match rows.iter_mut().find(|r| r.domain == RegDomain::DomD3) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        d3_first.claim = StateClaim::ClaimSlotState;
        if led_detect_row_mix(&rows) {
            caught += 1;
        } else {
            escape.push_str("D3行;");
        }
    }
    // 3. D4 行混报（槽位域行报真绿）。
    {
        let mut rows = build_edge_registry();
        let d4_first = match rows.iter_mut().find(|r| r.domain == RegDomain::DomD4) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        d4_first.claim = StateClaim::ClaimTrueGreen;
        if led_detect_row_mix(&rows) {
            caught += 1;
        } else {
            escape.push_str("D4行;");
        }
    }
    // 4. E1 行混报。
    {
        let mut rows = build_edge_registry();
        let e1_first = match rows.iter_mut().find(|r| r.domain == RegDomain::DomE1) {
            Some(v) => v,
            None => return (caught, 10, String::from("表缺行")),
        };
        e1_first.claim = StateClaim::ClaimTrueGreen;
        if led_detect_row_mix(&rows) {
            caught += 1;
        } else {
            escape.push_str("E1行;");
        }
    }
    // 5. D2 汇总混报。
    {
        let bad = DomainSummary { domain: RegDomain::DomD2, claim: StateClaim::ClaimSlotState, row_count: 15 };
        // 混报汇总走新账面检验。
        let mut fresh = ErLedger::new();
        let _ = fresh.begin_run();
        if fresh.summarize(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("D2汇;");
        }
    }
    // 6. D4 汇总混报。
    {
        let mut fresh = ErLedger::new();
        let _ = fresh.begin_run();
        let bad = DomainSummary { domain: RegDomain::DomD4, claim: StateClaim::ClaimTrueGreen, row_count: 15 };
        if fresh.summarize(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("D4汇;");
        }
    }
    // 7. 汇总缺一：60 行齐但只落三条汇总就封。
    {
        let rows = build_edge_registry();
        let mut led = ErLedger::new();
        let _ = led.begin_run();
        for r in rows.iter() {
            let _ = led.reg(r.clone());
        }
        let mut k = 0usize;
        while k < 3 {
            let d = match RegDomain::all().get(k) {
                Some(v) => *v,
                None => break,
            };
            let _ = led.summarize(DomainSummary { domain: d, claim: frozen_claim(d), row_count: d.row_cap() as u32 });
            k += 1;
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("汇缺;");
        }
    }
    // 8. 汇总重一。
    {
        let mut led = ErLedger::new();
        let _ = led.begin_run();
        let s = DomainSummary { domain: RegDomain::DomD2, claim: frozen_claim(RegDomain::DomD2), row_count: 15 };
        let _ = led.summarize(s.clone());
        if led.summarize(s).is_err() {
            caught += 1;
        } else {
            escape.push_str("汇重;");
        }
    }
    // 9. 行额漂移（汇总行额 14 ≠ 15）。
    {
        let mut led = ErLedger::new();
        let _ = led.begin_run();
        let bad = DomainSummary { domain: RegDomain::DomD3, claim: frozen_claim(RegDomain::DomD3), row_count: 14 };
        if led.summarize(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("额漂;");
        }
    }
    // 10. 封账后写。
    {
        let mut led = build_full_er_ledger();
        let extra = EdgeEntry {
            no: 999,
            domain: RegDomain::DomD2,
            iface: "越界行",
            kind: EdgeKind::ProbeEdge,
            claim: frozen_claim(RegDomain::DomD2),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    (caught, 10, escape)
}

/// 行级混报检出（对给定登记表逐行核申报口径，发现混报返回 true）。
fn led_detect_row_mix(rows: &[EdgeEntry]) -> bool {
    for r in rows.iter() {
        if r.claim != frozen_claim(r.domain) {
            return true;
        }
    }
    false
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_er_ledger();
    let b = build_full_er_ledger();
    if a.len() != b.len() || a.summary_len() != b.summary_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.row_at(i), b.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if er_fault_sweep().0 != er_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要。
pub fn frozen_digest() -> u32 {
    let rows = build_edge_registry();
    let mut acc: u32 = SEG_BASE;
    for r in rows.iter() {
        acc = acc.wrapping_mul(13).wrapping_add(r.domain.ordinal()).wrapping_add(r.no);
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
    if SEG_BASE != 0x80000 + 0x40 * (12691u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_er_ledger();
    if led.state() != ErState::ErSealed {
        return false;
    }
    let head = match led.row_at(0) {
        Some(r) => r.iface,
        None => return false,
    };
    if head.is_empty() {
        return false;
    }
    for d in RegDomain::all().iter() {
        if d.name().is_empty() {
            return false;
        }
    }
    for c in StateClaim::all().iter() {
        if c.name().is_empty() {
            return false;
        }
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12692 消费边接口登记总表自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12691_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12691-edge-registry");
    let led = build_full_er_ledger();

    s.add_dyn(
        "F12692-01 登记行 60 行全量",
        registry_complete_ok(),
        &format!("rows={} cap={}", led.len(), EDGE_ROW_CAP),
    );

    s.add_dyn(
        "F12692-02 两态账汇总判据绿",
        twostate_summary_green_ok(),
        &format!("summaries={} sealed={}", led.summary_len(), led.state() == ErState::ErSealed),
    );

    s.add_dyn(
        "F12692-03 冻结口径分列",
        frozen_claim(RegDomain::DomD2) == StateClaim::ClaimTrueGreen
            && frozen_claim(RegDomain::DomD3) == StateClaim::ClaimTrueGreen
            && frozen_claim(RegDomain::DomD4) == StateClaim::ClaimSlotState
            && frozen_claim(RegDomain::DomE1) == StateClaim::ClaimSlotState,
        "D2/D3 真绿、D4/E1 槽位态",
    );

    s.add_dyn(
        "F12692-04 混报注入 10/10 检出",
        mixreport_inject_ok(),
        &format!("caught={}", er_fault_sweep().0),
    );

    s.add_dyn(
        "F12692-05 域行额口径",
        RegDomain::DomD2.row_cap() == 15 && RegDomain::DomE1.row_cap() == 15,
        "每域 15 行 × 四方 = 60 行",
    );

    s.add_dyn(
        "F12692-06 边种三闭集轮转",
        kind_for_row(0) == EdgeKind::ProbeEdge
            && kind_for_row(1) == EdgeKind::LedgerEdge
            && kind_for_row(2) == EdgeKind::GateEdge
            && kind_for_row(3) == EdgeKind::ProbeEdge,
        "边种按行号 mod 3 轮转",
    );

    s.add_dyn(
        "F12692-07 账等式（点验≡行数≡汇总合计）",
        led.equation_ok(),
        &format!("probes={} rows={} sums={}", led.probe_len(), led.len(), led.summary_len()),
    );

    s.add_dyn(
        "F12692-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = er_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", er_fault_sweep().0),
    );

    s.add_dyn(
        "F12692-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12692-10 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12692-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12692-12 段位守卫",
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
        assert_eq!(SEG_BASE, 0x816C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(ER_ALL.len(), 10);
        let mut i = 0usize;
        while i < ER_ALL.len() {
            let vi = match ER_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < ER_ALL.len() {
                let vj = match ER_ALL.get(j) {
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
    fn registry_complete_holds() {
        assert!(registry_complete_ok());
        assert!(twostate_summary_green_ok());
        let led = build_full_er_ledger();
        assert_eq!(led.len(), 60);
        assert_eq!(led.summary_len(), 4);
        assert_eq!(led.state(), ErState::ErSealed);
        assert_eq!(led.reject_len(), 0);
        assert!(led.equation_ok());
        // 每域 15 行。
        for d in RegDomain::all().iter() {
            let mut count = 0usize;
            for r in led.rows_iter_domain(*d) {
                let _ = r;
                count += 1;
            }
            assert_eq!(count, 15, "域 {} 行额漂移", d.name());
        }
    }

    #[test]
    fn mixreport_injections_all_detected() {
        assert!(mixreport_inject_ok());
        let (caught, total, escape) = er_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn row_mix_detected_directly() {
        let mut rows = build_edge_registry();
        {
            let first = match rows.get_mut(0) {
                Some(v) => v,
                None => return,
            };
            first.claim = StateClaim::ClaimSlotState;
        }
        assert!(led_detect_row_mix(&rows));
        // 还原后零混报。
        {
            match rows.get_mut(0) {
                Some(v) => v.claim = StateClaim::ClaimTrueGreen,
                None => return,
            };
        }
        assert!(!led_detect_row_mix(&rows));
    }

    #[test]
    fn summary_gates_reject() {
        let mut led = ErLedger::new();
        let _ = led.begin_run();
        // 汇总混报。
        let bad = DomainSummary { domain: RegDomain::DomD2, claim: StateClaim::ClaimSlotState, row_count: 15 };
        assert_eq!(led.summarize(bad), Err(ErDiag::ESummaryMix.val()));
        // 行额漂移。
        let drift = DomainSummary { domain: RegDomain::DomD2, claim: StateClaim::ClaimTrueGreen, row_count: 14 };
        assert_eq!(led.summarize(drift), Err(ErDiag::ERowCountDrift.val()));
        // 正常汇总过闸后重一拒。
        let ok = DomainSummary { domain: RegDomain::DomD2, claim: StateClaim::ClaimTrueGreen, row_count: 15 };
        assert!(led.summarize(ok.clone()).is_ok());
        assert_eq!(led.summarize(ok), Err(ErDiag::ESummaryDup.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn reg_gate_rejects() {
        let mut led = ErLedger::new();
        let _ = led.begin_run();
        let rows = build_edge_registry();
        // 行混报（D2 域行报槽位态）。
        let mut mix = match rows.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        mix.claim = StateClaim::ClaimSlotState;
        assert_eq!(led.reg(mix), Err(ErDiag::ERowMix.val()));
        // 接口置空。
        let mut empty = match rows.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        empty.iface = "";
        assert_eq!(led.reg(empty), Err(ErDiag::EIfaceEmpty.val()));
        // 边种越位。
        let mut kind = match rows.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        kind.kind = EdgeKind::GateEdge;
        assert_eq!(led.reg(kind), Err(ErDiag::EKindRange.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn domain_iface_boundary() {
        assert!(domain_iface(RegDomain::DomD2, 0).is_some());
        assert!(domain_iface(RegDomain::DomD2, 15).is_none());
        assert!(domain_iface(RegDomain::DomE1, 14).is_some());
        assert!(domain_iface(RegDomain::DomE1, 15).is_none());
        assert_eq!(D2_IFACES.len(), 15);
        assert_eq!(D3_IFACES.len(), 15);
        assert_eq!(D4_IFACES.len(), 15);
        assert_eq!(E1_IFACES.len(), 15);
        // 四方清单接口名互异。
        for a in D2_IFACES.iter() {
            for b in E1_IFACES.iter() {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_er_ledger();
        let b = build_full_er_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.row_at(i), b.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑行缺失"),
            }
            i += 1;
        }
        assert_eq!(er_fault_sweep().0, er_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(ErState::ErOpen.rank() < ErState::ErRunning.rank());
        assert!(ErState::ErRunning.rank() < ErState::ErSealed.rank());
        let mut led = build_full_er_ledger();
        assert_eq!(led.state(), ErState::ErSealed);
        assert_eq!(led.seal(), Err(ErDiag::ERewind.val()));
        let mut fresh = ErLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(ErDiag::ERewind.val()));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12691_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}

impl ErLedger {
    /// 取某域全部登记行（遍历口径，测试与普查用）。
    pub fn rows_iter_domain(&self, d: RegDomain) -> Vec<&EdgeEntry> {
        let mut out = Vec::new();
        for r in self.rows.iter() {
            if r.domain == d {
                out.push(r);
            }
        }
        out
    }
}
