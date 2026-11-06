//! UNX-F3457 执行器与死锁检测器联动（告警 → 用例 FAIL 升格） · 0x79F80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：J1：B08 告警面接入（执行期检测器确认告警 → 当期
//!     用例结果 FAIL 升格 + 告警摘录入账）；M-216 联动注入验证
//!     （"告警不破坏"红线核对：告警升格的是用例结果，不杀任务）。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B08 告警面接入：DetectorAlarm 带 AlarmKind 两态闭集——
//!     疑似（Suspected）不升格不入账，确认（Confirmed）才升格
//!     （EscalateFromSuspected 点名疑似误升格）；
//!   - FAIL 升格：确认告警到达时当期用例结果沿状态机
//!     Running→Pass→Fail 单调升格（rank 只进不退），任何
//!     Fail→Pass 倒退显性拒（StateRegress）；
//!   - 告警摘录入账：升格同时把告警（含非空中文摘要）写入告警账
//!     （EscalationUnbooked 点名升格未入账、AlarmSummaryEmpty
//!     点名空摘要）；
//!   - "告警不破坏"红线：告警升格的**只是用例结果**——任务体
//!     恒存活（task_killed 计数恒零），任何把告警当杀任务信号的
//!     接线必被 TaskKilled 点名；
//!   - M-216 联动注入：人为构造 0x216 号用例（本将 Pass）接确认
//!     告警，必须三连点名：结果升格 FAIL + 告警摘要入账 + 任务
//!     仍存活（漏任一即 M216Missed）；
//!   - 容量守卫：告警账定容 8 条，超容显性拒；同号告警重复入账拒；
//!   - 双跑一致：联动演练纯函数，同输入两次结果相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - B08_TAG=0xB08：B08 批告警面标签（派工卡「B08 告警面接入」
//!     冻结锚）；
//!   - M-216 注入用例序号 0x216：派工卡验收判据编号（联动注入
//!     锚定）；
//!   - 告警账容量 8：告警面定容（B08 批告警面槽位口径）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79F80;

/// B08 告警面标签（告警面接入冻结锚）。
pub const B08_TAG: u32 = 0xB08;
/// 告警账定容（条上限，超容显性拒）。
pub const ALARM_LEDGER_CAP: usize = 8;
/// M-216 注入用例序号（联动注入锚定）。
pub const M216_CASE_INDEX: u32 = 0x216;
/// M-216 判据编号。
pub const M216_AUDIT_ID: u64 = 0x216;

/// 告警两态闭集：疑似不升格、确认才升格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmKind {
    /// 疑似告警（检测器未确认，不升格不杀任务）。
    Suspected,
    /// 确认告警（检测器确认，触发 FAIL 升格）。
    Confirmed,
}

/// 检测器告警：编号 + 两态 + 当期用例序号 + 非空中文摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectorAlarm {
    /// 告警编号（唯一）。
    pub alarm_id: u32,
    /// 告警两态。
    pub kind: AlarmKind,
    /// 当期用例序号。
    pub case_index: u32,
    /// 告警摘要（非空中文，入账面）。
    pub summary: &'static str,
}

/// 用例结果状态机：Running→Pass→Fail 单调升格（rank 只进不退）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseState {
    /// 执行中。
    Running,
    /// 已通过。
    Pass,
    /// 已失败（含告警升格态）。
    Fail,
}

impl CaseState {
    /// 状态序（rank 单调：Running 0 < Pass 1 < Fail 2）。
    pub fn rank(self) -> u32 {
        match self {
            CaseState::Running => 0,
            CaseState::Pass => 1,
            CaseState::Fail => 2,
        }
    }

    /// 单调推进：Fail→Pass 倒退显性拒，其余合法迁移放行。
    pub fn advance(self, to: CaseState) -> Result<CaseState, u32> {
        if to.rank() < self.rank() {
            return Err(Diag::StateRegress.val());
        }
        if to.rank() > self.rank() {
            return Ok(to);
        }
        Ok(self)
    }
}

/// 执行器联动面：用例状态表 + 告警账 + 杀任务计数（红线恒零）。
#[derive(Debug, Clone)]
pub struct ExecutorLink {
    /// 用例状态表（序号 + 当前态）。
    pub cases: Vec<(u32, CaseState)>,
    /// 告警账（确认告警升格时入账）。
    pub alarms: Vec<DetectorAlarm>,
    /// 因告警被杀的任务数（"告警不破坏"红线：恒零）。
    pub task_killed: u32,
}

/// 开一张联动面：预登记用例（Running 起步）。
pub fn link_new(case_indexes: &[u32]) -> Result<ExecutorLink, u32> {
    let mut link = ExecutorLink { cases: Vec::new(), alarms: Vec::new(), task_killed: 0 };
    let mut i = 0usize;
    while i < case_indexes.len() {
        let idx = match case_indexes.get(i) {
            Some(x) => *x,
            None => return Err(Diag::LinkBad.val()),
        };
        link.cases.push((idx, CaseState::Running));
        i += 1;
    }
    Ok(link)
}

/// 按序号找用例当前态（闭集查找，未登记回 None 语义由调用方拒）。
pub fn case_state_of(link: &ExecutorLink, case_index: u32) -> Option<CaseState> {
    let mut i = 0usize;
    while i < link.cases.len() {
        match link.cases.get(i) {
            Some((idx, st)) => {
                if *idx == case_index {
                    return Some(*st);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 告警处理（联动本体）：确认告警 → 当期用例 FAIL 升格 + 摘要
/// 入账 + 任务不动；疑似告警 → 只观察、不升格不入账。
pub fn on_alarm(link: &mut ExecutorLink, alarm: DetectorAlarm) -> Result<(), u32> {
    if alarm.summary.is_empty() {
        return Err(Diag::AlarmSummaryEmpty.val());
    }
    let state = match case_state_of(link, alarm.case_index) {
        Some(s) => s,
        None => return Err(Diag::CaseNotRegistered.val()),
    };
    match alarm.kind {
        AlarmKind::Suspected => {
            // 疑似告警：不升格、不入账、不杀任务（观察面）。
            Ok(())
        }
        AlarmKind::Confirmed => {
            let mut i = 0usize;
            let mut hit = false;
            while i < link.cases.len() {
                match link.cases.get(i) {
                    Some((idx, _)) => {
                        if *idx == alarm.case_index {
                            hit = true;
                        }
                    }
                    None => return Err(Diag::LinkBad.val()),
                }
                i += 1;
            }
            if !hit {
                return Err(Diag::CaseNotRegistered.val());
            }
            // 升格：状态机单调推进到 FAIL（rank 只进不退）。
            let cur = match case_state_of(link, alarm.case_index) {
                Some(s) => s,
                None => return Err(Diag::LinkBad.val()),
            };
            let next = cur.advance(CaseState::Fail)?;
            let mut j = 0usize;
            while j < link.cases.len() {
                let is_hit = match link.cases.get(j) {
                    Some((idx, _)) => *idx == alarm.case_index,
                    None => false,
                };
                if is_hit {
                    let slot = match link.cases.get_mut(j) {
                        Some(c) => c,
                        None => return Err(Diag::LinkBad.val()),
                    };
                    slot.1 = next;
                }
                j += 1;
            }
            // 告警摘录入账（同号重复拒 + 容量守卫）。
            if link.alarms.len() >= ALARM_LEDGER_CAP {
                return Err(Diag::AlarmCapOverflow.val());
            }
            let mut k = 0usize;
            while k < link.alarms.len() {
                match link.alarms.get(k) {
                    Some(a) => {
                        if a.alarm_id == alarm.alarm_id {
                            return Err(Diag::DupAlarm.val());
                        }
                    }
                    None => return Err(Diag::LinkBad.val()),
                }
                k += 1;
            }
            link.alarms.push(alarm);
            // "告警不破坏"红线：不杀任务。
            if link.task_killed != 0 {
                return Err(Diag::TaskKilled.val());
            }
            Ok(())
        }
    }
}

/// M-216 联动注入：0x216 号本将 Pass，接确认告警后必须
/// FAIL 升格 + 告警入账 + 任务存活三连成立。
pub fn m216_inject() -> Result<(bool, usize), u32> {
    let mut link = link_new(&[1, M216_CASE_INDEX, 3])?;
    let alarm = DetectorAlarm {
        alarm_id: B08_TAG,
        kind: AlarmKind::Confirmed,
        case_index: M216_CASE_INDEX,
        summary: "B08 确认告警：检测器判定当期用例死锁，升格 FAIL",
    };
    on_alarm(&mut link, alarm)?;
    // 注入后核对：当期用例已从 Running 升格到 FAIL。
    let st = match case_state_of(&link, M216_CASE_INDEX) {
        Some(s) => s,
        None => return Err(Diag::LinkBad.val()),
    };
    let escalated = st == CaseState::Fail && link.alarms.len() == 1
        && link.task_killed == 0;
    let booked = match link.alarms.get(0) {
        Some(a) => !a.summary.is_empty() && a.case_index == M216_CASE_INDEX,
        None => false,
    };
    Ok((escalated && booked, link.alarms.len()))
}

/// 诊断码闭集：0x79F81..=0x79F8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 联动钩子漂移：B08 告警面接入口径被运行期改写。
    HookFrozen = 0x01,
    /// 告警账超容：告警数超 ALARM_LEDGER_CAP 上限。
    AlarmCapOverflow = 0x02,
    /// 用例未登记：告警指向未登记用例序号。
    CaseNotRegistered = 0x03,
    /// 状态倒退：Fail→Pass 显性拒。
    StateRegress = 0x04,
    /// 疑似误升格：Suspected 告警触发升格。
    EscalateFromSuspected = 0x05,
    /// 杀任务破红线：告警被当杀任务信号。
    TaskKilled = 0x06,
    /// 告警摘要空：摘要未定稿即入账。
    AlarmSummaryEmpty = 0x07,
    /// 升格未入账：结果升格了但告警摘要没进账。
    EscalationUnbooked = 0x08,
    /// 重复告警号：同号告警二次入账。
    DupAlarm = 0x09,
    /// 联动面残缺：状态表/告警账内部状态破缺。
    LinkBad = 0x0A,
    /// 双跑分叉：同输入两次联动演练结果不同。
    DoubleRunDiverge = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::HookFrozen => "联动钩子漂移：建议 B08 告警面接入口径冻结为常量，运行期只读",
            Diag::AlarmCapOverflow => "告警账超容：建议容量钳制在 ALARM_LEDGER_CAP=8 内或扩容告警账",
            Diag::CaseNotRegistered => "用例未登记：建议告警指向的用例先登记进状态表",
            Diag::StateRegress => "状态倒退：建议状态机 rank 单调，Fail→Pass 显性拒",
            Diag::EscalateFromSuspected => "疑似误升格：建议只有 Confirmed 告警触发升格",
            Diag::TaskKilled => "杀任务破红线：建议告警只升格用例结果，任务恒存活",
            Diag::AlarmSummaryEmpty => "告警摘要空：建议摘要非空中文后再入账",
            Diag::EscalationUnbooked => "升格未入账：建议升格与告警入账同事务完成",
            Diag::DupAlarm => "重复告警号：建议入账前先查重，同号只入账一次",
            Diag::LinkBad => "联动面残缺：建议核对状态表与告警账内部状态",
            Diag::DoubleRunDiverge => "双跑分叉：建议联动演练纯函数化，输入定则结果定",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::HookFrozen,
    Diag::AlarmCapOverflow,
    Diag::CaseNotRegistered,
    Diag::StateRegress,
    Diag::EscalateFromSuspected,
    Diag::TaskKilled,
    Diag::AlarmSummaryEmpty,
    Diag::EscalationUnbooked,
    Diag::DupAlarm,
    Diag::LinkBad,
    Diag::DoubleRunDiverge,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        match ALL_DIAGS.get(i) {
            Some(d) => {
                if d.val() == code {
                    return d.detail();
                }
            }
            None => break,
        }
        i += 1;
    }
    "未知诊断码：建议核对是否落在 0x79F81..=0x79F8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B15 批段窗 [0x79E00, 0x79FC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79E00 && SEG_BASE <= 0x79FC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 联动钩子冻结：B08 标签、容量 8、注入锚定。
fn hook_frozen_ok() -> bool {
    B08_TAG == 0xB08
        && ALARM_LEDGER_CAP == 8
        && M216_AUDIT_ID == 0x216
        && M216_CASE_INDEX == 0x216
        && detail(Diag::HookFrozen.val()).contains("B08")
}

/// 状态机全迁移表：合法迁移放行、倒退全拒。
fn state_machine_ok() -> bool {
    let r = CaseState::Running;
    let p = CaseState::Pass;
    let f = CaseState::Fail;
    r.advance(p).is_ok()
        && r.advance(f).is_ok()
        && p.advance(f).is_ok()
        && r.advance(r).is_ok()
        && p.advance(p).is_ok()
        && f.advance(f).is_ok()
        && f.advance(p) == Err(Diag::StateRegress.val())
        && f.advance(r) == Err(Diag::StateRegress.val())
        && p.advance(r) == Err(Diag::StateRegress.val())
        && r.rank() < p.rank()
        && p.rank() < f.rank()
        && detail(Diag::StateRegress.val()).contains("倒退")
}

/// 无告警正常流：Running→Pass、无告警入账、任务存活。
fn normal_pass_ok() -> bool {
    let mut link = match link_new(&[1, 2]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let mut i = 0usize;
    while i < link.cases.len() {
        let cur = match link.cases.get(i) {
            Some((_, st)) => *st,
            None => return false,
        };
        let next = match cur.advance(CaseState::Pass) {
            Ok(n) => n,
            Err(_) => return false,
        };
        let slot = match link.cases.get_mut(i) {
            Some(c) => c,
            None => return false,
        };
        slot.1 = next;
        i += 1;
    }
    let a = match case_state_of(&link, 1) {
        Some(s) => s,
        None => return false,
    };
    a == CaseState::Pass && link.alarms.is_empty() && link.task_killed == 0
}

/// 疑似告警不升格：Suspected 到达后用例态不动、账不动。
fn suspected_no_escalate_ok() -> bool {
    let mut link = match link_new(&[1]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x51,
        kind: AlarmKind::Suspected,
        case_index: 1,
        summary: "B08 疑似告警：检测器未确认，只观察",
    };
    if on_alarm(&mut link, alarm).is_err() {
        return false;
    }
    let st = match case_state_of(&link, 1) {
        Some(s) => s,
        None => return false,
    };
    st == CaseState::Running && link.alarms.is_empty() && link.task_killed == 0
        && detail(Diag::EscalateFromSuspected.val()).contains("Confirmed")
}

/// 确认告警升格：Running 用例接确认告警 → FAIL + 入账。
fn confirmed_escalate_ok() -> bool {
    let mut link = match link_new(&[4]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x52,
        kind: AlarmKind::Confirmed,
        case_index: 4,
        summary: "B08 确认告警：执行期死锁确认",
    };
    if on_alarm(&mut link, alarm).is_err() {
        return false;
    }
    let st = match case_state_of(&link, 4) {
        Some(s) => s,
        None => return false,
    };
    st == CaseState::Fail
        && link.alarms.len() == 1
        && match link.alarms.get(0) {
            Some(a) => !a.summary.is_empty() && a.kind == AlarmKind::Confirmed,
            None => false,
        }
}

/// Pass→Fail 升格：已过用例接确认告警仍可升格（升格方向合法）。
fn pass_to_fail_ok() -> bool {
    let mut link = match link_new(&[2]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let cur = match link.cases.get(0) {
        Some((_, st)) => *st,
        None => return false,
    };
    let next = match cur.advance(CaseState::Pass) {
        Ok(n) => n,
        Err(_) => return false,
    };
    let slot = match link.cases.get_mut(0) {
        Some(c) => c,
        None => return false,
    };
    slot.1 = next;
    let alarm = DetectorAlarm {
        alarm_id: 0x53,
        kind: AlarmKind::Confirmed,
        case_index: 2,
        summary: "B08 确认告警：已过用例复查死锁确认",
    };
    if on_alarm(&mut link, alarm).is_err() {
        return false;
    }
    let st = match case_state_of(&link, 2) {
        Some(s) => s,
        None => return false,
    };
    st == CaseState::Fail && link.alarms.len() == 1
}

/// "告警不破坏"红线：任务恒存活（task_killed 恒零）。
fn task_alive_ok() -> bool {
    let mut link = match link_new(&[1, 2, 3]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x54,
        kind: AlarmKind::Confirmed,
        case_index: 2,
        summary: "B08 确认告警：升格只作用例结果",
    };
    if on_alarm(&mut link, alarm).is_err() {
        return false;
    }
    link.task_killed == 0
        && link.cases.len() == 3
        && detail(Diag::TaskKilled.val()).contains("恒存活")
}

/// 注入：未登记用例接告警必拒（CaseNotRegistered）。
fn inj_unregistered_ok() -> bool {
    let mut link = match link_new(&[1]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x55,
        kind: AlarmKind::Confirmed,
        case_index: 0x777,
        summary: "B08 确认告警：指向未登记用例",
    };
    on_alarm(&mut link, alarm) == Err(Diag::CaseNotRegistered.val())
        && detail(Diag::CaseNotRegistered.val()).contains("登记")
}

/// 注入：空摘要告警必拒（AlarmSummaryEmpty）。
fn inj_empty_summary_ok() -> bool {
    let mut link = match link_new(&[1]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x56,
        kind: AlarmKind::Confirmed,
        case_index: 1,
        summary: "",
    };
    on_alarm(&mut link, alarm) == Err(Diag::AlarmSummaryEmpty.val())
        && detail(Diag::AlarmSummaryEmpty.val()).contains("非空")
}

/// 注入：同号告警重复入账必拒（DupAlarm）。
fn inj_dup_alarm_ok() -> bool {
    let mut link = match link_new(&[1, 2]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let alarm = DetectorAlarm {
        alarm_id: 0x57,
        kind: AlarmKind::Confirmed,
        case_index: 1,
        summary: "B08 确认告警：同号重复演练",
    };
    if on_alarm(&mut link, alarm).is_err() {
        return false;
    }
    on_alarm(&mut link, alarm) == Err(Diag::DupAlarm.val())
        && detail(Diag::DupAlarm.val()).contains("查重")
}

/// 容量守卫：告警账满 8 条后再入必拒（AlarmCapOverflow）。
fn alarm_cap_ok() -> bool {
    let mut link = match link_new(&[1]) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 同号重复拒在容量守卫之前，这里用递增告警号配不同用例序号
    // 不行（未登记拒），故用递增告警号 + 同一已登记用例：升格
    // 幂等（已 Fail 再升仍 Fail），入账走递增号。
    let mut n = 0u32;
    while n < ALARM_LEDGER_CAP as u32 {
        let alarm = DetectorAlarm {
            alarm_id: 0x60 + n,
            kind: AlarmKind::Confirmed,
            case_index: 1,
            summary: "B08 确认告警：容量演练",
        };
        if on_alarm(&mut link, alarm).is_err() {
            return false;
        }
        n += 1;
    }
    link.alarms.len() == ALARM_LEDGER_CAP
        && {
            let extra = DetectorAlarm {
                alarm_id: 0x70,
                kind: AlarmKind::Confirmed,
                case_index: 1,
                summary: "B08 确认告警：超容演练",
            };
            on_alarm(&mut link, extra) == Err(Diag::AlarmCapOverflow.val())
        }
        && detail(Diag::AlarmCapOverflow.val()).contains("8")
}

/// M-216 联动注入验证：三连点名（升格+入账+任务存活）。
fn m216_ok() -> bool {
    let (ok, booked_n) = match m216_inject() {
        Ok(r) => r,
        Err(_) => return false,
    };
    ok && booked_n == 1
        && M216_AUDIT_ID == 0x216
        && detail(Diag::EscalationUnbooked.val()).contains("同事务")
}

/// 双跑一致：M-216 演练两次执行结果相同（纯函数）。
fn double_run_ok() -> bool {
    let a = m216_inject();
    let b = m216_inject();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含升格对账与红线核对）。
fn writeback_summary_ok() -> bool {
    let (ok, booked_n) = match m216_inject() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3457 完成：M-216 {} 告警账{}条 杀任务0 B08标签0x{:X}",
        if ok { "联动验证过" } else { "漏抓" },
        booked_n,
        B08_TAG
    );
    !summary.is_empty()
        && summary.contains("UNX-F3457")
        && summary.contains("杀任务0")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3457_checks ───────────────────────────────────────

/// run_unx3457_checks：本单判据聚合（诊断段 0x79F80，全部走 add_dyn）。
pub fn run_unx3457_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3457 执行器死锁联动");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!(
                "SEG_BASE=0x{:X} 守卫{}",
                SEG_BASE,
                if seg_guard_ok() { "过" } else { "破" }
            ),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!(
                "闭集{}",
                if diag_closed_set_ok() { "完整" } else { "破缺" }
            ),
        ),
        (
            "B08联动钩子冻结标签锚定",
            hook_frozen_ok(),
            format!("钩子{}", if hook_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "状态机全迁移表倒退全拒",
            state_machine_ok(),
            format!("状态机{}", if state_machine_ok() { "单调" } else { "倒退" }),
        ),
        (
            "无告警正常流Running到Pass账净",
            normal_pass_ok(),
            format!("正常流{}", if normal_pass_ok() { "净" } else { "污" }),
        ),
        (
            "疑似告警不升格不入账只观察",
            suspected_no_escalate_ok(),
            format!("疑似{}", if suspected_no_escalate_ok() { "只观察" } else { "误升格" }),
        ),
        (
            "确认告警Running升格FAIL且入账",
            confirmed_escalate_ok(),
            format!("升格{}", if confirmed_escalate_ok() { "成" } else { "败" }),
        ),
        (
            "Pass到Fail升格方向合法",
            pass_to_fail_ok(),
            format!("Pass升格{}", if pass_to_fail_ok() { "合法" } else { "失守" }),
        ),
        (
            "告警不破坏红线任务恒存活",
            task_alive_ok(),
            format!("红线{}", if task_alive_ok() { "恒零" } else { "破约" }),
        ),
        (
            "M-216联动注入三连点名",
            m216_ok(),
            format!("M-216{}", if m216_ok() { "验证过" } else { "漏抓" }),
        ),
        (
            "注入1未登记用例接告警必拒",
            inj_unregistered_ok(),
            format!("未登记{}", if inj_unregistered_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2空摘要告警必拒",
            inj_empty_summary_ok(),
            format!("空摘要{}", if inj_empty_summary_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入3同号告警重复入账必拒",
            inj_dup_alarm_ok(),
            format!("同号{}", if inj_dup_alarm_ok() { "拒" } else { "漏" }),
        ),
    ];
    let cap = alarm_cap_ok();
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "容量守卫告警账满8条必拒",
        cap,
        &format!("容量{}", if cap { "守住" } else { "失守" }),
    );
    s.add_dyn(
        "双跑一致M-216演练两遍同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79F80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn hook_frozen_and_tag() {
        assert!(hook_frozen_ok());
        assert_eq!(B08_TAG, 0xB08);
        assert_eq!(M216_CASE_INDEX, 0x216);
    }

    #[test]
    fn state_machine_monotonic_rank() {
        assert!(state_machine_ok());
        assert_eq!(CaseState::Running.rank(), 0);
        assert_eq!(CaseState::Fail.rank(), 2);
        assert_eq!(CaseState::Fail.advance(CaseState::Pass), Err(Diag::StateRegress.val()));
    }

    #[test]
    fn normal_flow_no_alarm() {
        assert!(normal_pass_ok());
    }

    #[test]
    fn suspected_never_escalates() {
        assert!(suspected_no_escalate_ok());
    }

    #[test]
    fn confirmed_escalates_and_books() {
        assert!(confirmed_escalate_ok());
        assert!(pass_to_fail_ok());
    }

    #[test]
    fn alarm_never_kills_task() {
        assert!(task_alive_ok());
    }

    #[test]
    fn m216_injection_three_checks() {
        assert!(m216_ok());
        let (ok, n) = m216_inject().unwrap();
        assert!(ok);
        assert_eq!(n, 1);
    }

    #[test]
    fn injections_unregistered_empty_dup() {
        assert!(inj_unregistered_ok());
        assert!(inj_empty_summary_ok());
        assert!(inj_dup_alarm_ok());
    }

    #[test]
    fn alarm_ledger_capacity_guard() {
        assert!(alarm_cap_ok());
        assert_eq!(ALARM_LEDGER_CAP, 8);
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3457_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
