//! UNX-F0734 · 跟随 SLA 违约告警（>7 日拦截） · 诊断段 0x5b40
//! 判据原文摘录：UNX-F0734-J1 SLA 违约检出 100%（超时注入必告警）且告警账行
//! 四要素齐、违约未处置拦截收口；完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. SLA 时限写死为 7 个自然日（>7 日即违约），来源：跟随面服务级协议 B37 定版，
//!    常量不落配置、不留后门。
//! 2. 违约检出 100% 口径：任何一条 elapsed_days > SLA_DAYS 的跟随单，
//!    扫描后必须产生一条告警账行，漏检即断言失败。
//! 3. 告警账行四要素：单号 / 违约天数 / 告警级别 / 处置状态，四者缺一即账行无效，
//!    无效账行不参与守恒。
//! 4. 违约未处置拦截收口：存在 Unhandled 态告警时，收口函数必须拒绝，
//!    拦截口径全有全无。
//! 5. 定容数组账本 + 守恒恒等式：handled+escalated+unhandled == alarm_count。
//! 6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!    诊断段 0x5b40 为本单独占。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5b40;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_TICKET_DUP: u16 = SEG_BASE + 1; // 跟随单号重复登记
const E_TICKET_FULL: u16 = SEG_BASE + 2; // 跟随单账容量已满
const E_TICKET_NOTFOUND: u16 = SEG_BASE + 3; // 跟随单号不存在
const E_DAYS_BACK: u16 = SEG_BASE + 4; // 流转天数为负或非法
const E_NO_VIOLATION: u16 = SEG_BASE + 5; // 未超时限，不构成违约，拒绝出告警
const E_ALARM_DUP: u16 = SEG_BASE + 6; // 同单重复告警（未处置期禁止二次告警）
const E_ALARM_NOTFOUND: u16 = SEG_BASE + 7; // 告警行号不存在
const E_ELEM_MISSING: u16 = SEG_BASE + 8; // 告警账行四要素不齐
const E_STATE_EDGE: u16 = SEG_BASE + 9; // 处置状态机非法边
const E_CLOSE_BLOCKED: u16 = SEG_BASE + 10; // 收口被拦：存在未处置违约
const E_BAL: u16 = SEG_BASE + 11; // 守恒恒等式破裂（三态计数不守恒）
const E_SEALED: u16 = SEG_BASE + 12; // SLA 账已封账，拒绝再写
const E_NO_ALARM: u16 = SEG_BASE + 13; // 无告警可收口（空账）
const E_ESCALATE_TWICE: u16 = SEG_BASE + 14; // 升级处置重复触发

/// SLA 时限：7 个自然日（判据原文口径，写死）。
pub const SLA_DAYS: u32 = 7;

/// 跟随单账定容：64 单。
pub const MAX_TICKETS: usize = 64;

/// 告警账定容：与单账同容。
pub const MAX_ALARMS: usize = 64;

/// 告警四要素位图：bit0=单号 bit1=违约天数 bit2=告警级别 bit3=处置状态。
pub const ELEM_ID: u8 = 0b0001;
pub const ELEM_DAYS: u8 = 0b0010;
pub const ELEM_LEVEL: u8 = 0b0100;
pub const ELEM_STATE: u8 = 0b1000;
pub const ELEM_ALL: u8 = 0b1111;

// ── FNV-1a 64（封账哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TICKET_DUP => "跟随单号重复：单号是账内唯一键，先查账再登记",
        E_TICKET_FULL => "单账已满 64 条：分账或归档旧单后重试",
        E_TICKET_NOTFOUND => "跟随单不存在：登记与查询须用已登记单号",
        E_DAYS_BACK => "流转天数非法：自然日计数不允许负值",
        E_NO_VIOLATION => "未超 7 日时限：正常单不出告警，勿人工造警",
        E_ALARM_DUP => "同单重复告警：未处置期内一单只挂一条告警",
        E_ALARM_NOTFOUND => "告警行不存在：按告警行号操作",
        E_ELEM_MISSING => "四要素不齐：单号/天数/级别/状态必须全落账",
        E_STATE_EDGE => "处置状态机非法边：合法边见状态枚举注释",
        E_CLOSE_BLOCKED => "收口被拦：存在未处置违约，先处置再收口",
        E_BAL => "三态计数不守恒：handled+escalated+unhandled 必须等于告警总数",
        E_SEALED => "SLA 账已封账：封账后只读",
        E_NO_ALARM => "无告警可收口：账内零告警时收口无意义",
        E_ESCALATE_TWICE => "升级处置重复触发：一条告警只允许升级一次",
        _ => "未知诊断码",
    }
}

// ── 告警级别 ─────────────────────────────────────────────────
/// 告警级别：按违约深度分档（确定性映射，无人工调级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlarmLevel {
    /// 超时 1~2 日：低危提示。
    Low,
    /// 超时 3~6 日：中危催办。
    Mid,
    /// 超时 ≥7 日：高危升级（注意：>7 日即违约，7 日含当日即高危起点）。
    High,
}

impl AlarmLevel {
    /// 按违约天数确定性定级。
    pub fn by_overdue(days: u32) -> AlarmLevel {
        if days >= 7 {
            AlarmLevel::High
        } else if days >= 3 {
            AlarmLevel::Mid
        } else {
            AlarmLevel::Low
        }
    }

    /// 级别编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            AlarmLevel::Low => 1,
            AlarmLevel::Mid => 2,
            AlarmLevel::High => 3,
        }
    }

    /// 中文级别名（报告行渲染用）。
    pub fn zh(self) -> &'static str {
        match self {
            AlarmLevel::Low => "低危",
            AlarmLevel::Mid => "中危",
            AlarmLevel::High => "高危",
        }
    }
}

// ── 处置状态机 ───────────────────────────────────────────────
/// 告警处置状态：合法边 Unhandled→Handled；Unhandled→Escalated→Handled。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleState {
    /// 未处置（拦截收口的触发态）。
    Unhandled,
    /// 已升级（中间态，仍计未清）。
    Escalated,
    /// 已处置（终态，放行收口）。
    Handled,
}

impl HandleState {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            HandleState::Unhandled => 0,
            HandleState::Escalated => 1,
            HandleState::Handled => 2,
        }
    }

    /// 是否仍属未清（Escalated 也拦收口）。
    pub fn outstanding(self) -> bool {
        !matches!(self, HandleState::Handled)
    }
}

// ── 跟随单 ───────────────────────────────────────────────────
/// 一条跟随 SLA 单：登记时挂死线，扫描时按流转天数判违约。
#[derive(Clone, Debug)]
pub struct FollowTicket {
    /// 单号（账内唯一，1 起连续）。
    pub ticket_id: u32,
    /// 主题描述。
    pub subject: String,
    /// 登记日序（账内相对日，0 起）。
    pub opened_day: u32,
    /// 是否已出告警（一单至多一条在挂告警）。
    pub alarmed: bool,
    /// 单据是否已闭环（与告警处置联动）。
    pub closed: bool,
}

// ── 告警账行（四要素实体）─────────────────────────────────────
/// 一条告警账行：四要素齐备才算有效行。
#[derive(Clone, Debug)]
pub struct AlarmRow {    /// 告警行号（1 起连续）。
    pub alarm_id: u32,
    /// 要素一：关联跟随单号。
    pub ticket_id: u32,
    /// 要素二：违约天数（>SLA_DAYS）。
    pub overdue_days: u32,
    /// 要素三：告警级别。
    pub level: AlarmLevel,
    /// 要素四：处置状态。
    pub state: HandleState,
    /// 四要素落账位图（必须 == ELEM_ALL 才有效）。
    pub elems: u8,
}

impl AlarmRow {
    /// 四要素有效性判定。
    pub fn valid(&self) -> bool {
        self.elems == ELEM_ALL
            && self.ticket_id != 0
            && self.overdue_days > SLA_DAYS
    }
}

// ── SLA 账（定容账本主体）────────────────────────────────────
/// 跟随 SLA 违约告警账：单账 + 告警账双册，拦截收口的唯一事实源。
pub struct SlaLedger {
    tickets: [Option<FollowTicket>; MAX_TICKETS],
    alarms: [Option<AlarmRow>; MAX_ALARMS],
    /// 单账条数。
    ticket_count: usize,
    /// 告警账条数。
    alarm_count: usize,
    /// 三态计数：已处置 / 已升级（未清）/ 未处置。
    handled: u32,
    escalated: u32,
    unhandled: u32,
    /// 是否封账。
    sealed: bool,
}

impl SlaLedger {
    /// 开账。
    pub fn new() -> SlaLedger {
        SlaLedger {
            tickets: core::array::from_fn(|_| None),
            alarms: core::array::from_fn(|_| None),
            ticket_count: 0,
            alarm_count: 0,
            handled: 0,
            escalated: 0,
            unhandled: 0,
            sealed: false,
        }
    }

    /// 登记跟随单：单号自动连续分配，opened_day 即死线起点。
    pub fn open_ticket(&mut self, subject: &str, opened_day: u32) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.ticket_count >= MAX_TICKETS {
            return Err(E_TICKET_FULL);
        }
        if subject.is_empty() {
            return Err(E_TICKET_NOTFOUND);
        }
        let ticket_id = (self.ticket_count + 1) as u32;
        self.tickets[self.ticket_count] = Some(FollowTicket {
            ticket_id,
            subject: subject.to_string(),
            opened_day,
            alarmed: false,
            closed: false,
        });
        self.ticket_count += 1;
        Ok(ticket_id)
    }

    /// 单定位。
    fn slot_of(&self, ticket_id: u32) -> Result<usize, u16> {
        if ticket_id == 0 || ticket_id as usize > self.ticket_count {
            return Err(E_TICKET_NOTFOUND);
        }
        Ok((ticket_id - 1) as usize)
    }

    /// 告警定位。
    fn alarm_slot(&self, alarm_id: u32) -> Result<usize, u16> {
        if alarm_id == 0 || alarm_id as usize > self.alarm_count {
            return Err(E_ALARM_NOTFOUND);
        }
        Ok((alarm_id - 1) as usize)
    }

    /// 违约扫描：对指定单按当日序注入流转天数，超 7 日必出告警。
    /// 返回告警行号；未超时限返回 E_NO_VIOLATION。
    pub fn scan_ticket(&mut self, ticket_id: u32, today: u32) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.slot_of(ticket_id)?;
        let opened_day = {
            let t = self.tickets[s].as_ref().ok_or(E_TICKET_NOTFOUND)?;
            if t.alarmed {
                return Err(E_ALARM_DUP);
            }
            t.opened_day
        };
        if today < opened_day {
            return Err(E_DAYS_BACK);
        }
        let elapsed = today - opened_day;
        if elapsed <= SLA_DAYS {
            return Err(E_NO_VIOLATION);
        }
        let overdue_days = elapsed;
        if self.alarm_count >= MAX_ALARMS {
            return Err(E_TICKET_FULL);
        }
        let alarm_id = (self.alarm_count + 1) as u32;
        let level = AlarmLevel::by_overdue(overdue_days);
        self.alarms[self.alarm_count] = Some(AlarmRow {
            alarm_id,
            ticket_id,
            overdue_days,
            level,
            state: HandleState::Unhandled,
            elems: ELEM_ID | ELEM_DAYS | ELEM_LEVEL | ELEM_STATE,
        });
        self.alarm_count += 1;
        self.unhandled += 1;
        let t = self.tickets[s].as_mut().ok_or(E_TICKET_NOTFOUND)?;
        t.alarmed = true;
        Ok(alarm_id)
    }

    /// 全账扫描：逐单注入同一当日序，返回全部新告警行号。
    /// 违约检出 100% 口径：扫描后每条超时单必有告警。
    pub fn scan_all(&mut self, today: u32) -> Result<Vec<u32>, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let mut raised: Vec<u32> = Vec::new();
        let mut i = 1u32;
        while i <= self.ticket_count as u32 {
            match self.scan_ticket(i, today) {
                Ok(aid) => raised.push(aid),
                Err(E_NO_VIOLATION) | Err(E_ALARM_DUP) => {}
                Err(c) => return Err(c),
            }
            i += 1;
        }
        Ok(raised)
    }

    /// 处置：Unhandled→Handled（合法边一）。
    pub fn handle(&mut self, alarm_id: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.alarm_slot(alarm_id)?;
        let a = self.alarms[s].as_mut().ok_or(E_ALARM_NOTFOUND)?;
        if !a.valid() {
            return Err(E_ELEM_MISSING);
        }
        if a.state != HandleState::Unhandled {
            return Err(E_STATE_EDGE);
        }
        a.state = HandleState::Handled;
        let ticket_id = a.ticket_id;
        self.unhandled -= 1;
        self.handled += 1;
        // 单据联动闭环。
        let ts = self.slot_of(ticket_id)?;
        if let Some(t) = self.tickets[ts].as_mut() {
            t.closed = true;
        }
        Ok(())
    }

    /// 升级：Unhandled→Escalated（合法边二）。
    pub fn escalate(&mut self, alarm_id: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.alarm_slot(alarm_id)?;
        let a = self.alarms[s].as_mut().ok_or(E_ALARM_NOTFOUND)?;
        if a.state != HandleState::Unhandled {
            return Err(E_ESCALATE_TWICE);
        }
        a.state = HandleState::Escalated;
        self.unhandled -= 1;
        self.escalated += 1;
        Ok(())
    }

    /// 升级后回补处置：Escalated→Handled（合法边三）。
    pub fn resolve_escalated(&mut self, alarm_id: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        let s = self.alarm_slot(alarm_id)?;
        let a = self.alarms[s].as_mut().ok_or(E_ALARM_NOTFOUND)?;
        if a.state != HandleState::Escalated {
            return Err(E_STATE_EDGE);
        }
        a.state = HandleState::Handled;
        self.escalated -= 1;
        self.handled += 1;
        Ok(())
    }

    /// 守恒恒等式：handled+escalated+unhandled == alarm_count。
    pub fn conserved(&self) -> bool {
        self.handled + self.escalated + self.unhandled == self.alarm_count as u32
    }

    /// 收口：无未清告警且有账可收，否则拦截。
    pub fn close_out(&self) -> Result<u64, u16> {
        if self.alarm_count == 0 {
            return Err(E_NO_ALARM);
        }
        if !self.conserved() {
            return Err(E_BAL);
        }
        // 违约未处置拦截收口：任一未清即拒。
        let mut i = 0usize;
        while i < self.alarm_count {
            if let Some(a) = &self.alarms[i] {
                if a.state.outstanding() {
                    return Err(E_CLOSE_BLOCKED);
                }
            }
            i += 1;
        }
        let mut buf: Vec<u8> = Vec::new();
        i = 0;
        while i < self.alarm_count {
            if let Some(a) = &self.alarms[i] {
                buf.extend_from_slice(a.alarm_id.to_le_bytes().as_slice());
                buf.extend_from_slice(a.ticket_id.to_le_bytes().as_slice());
                buf.extend_from_slice(a.overdue_days.to_le_bytes().as_slice());
                buf.push(a.level.code());
                buf.push(a.state.code());
            }
            i += 1;
        }
        Ok(fnv1a64(&buf))
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.close_out()?;
        self.sealed = true;
        Ok(self.close_out()?)
    }

    /// 告警账只读行数。
    pub fn alarm_len(&self) -> usize {
        self.alarm_count
    }

    /// 告警行只读快照（四要素审计取数用）。
    pub fn get_alarm(&self, alarm_id: u32) -> Result<AlarmRow, u16> {
        let s = self.alarm_slot(alarm_id)?;
        match &self.alarms[s] {
            Some(a) => Ok(a.clone()),
            None => Err(E_ALARM_NOTFOUND),
        }
    }

    /// 单账只读行数。
    pub fn ticket_len(&self) -> usize {
        self.ticket_count
    }

    /// 未清告警行号清单。
    pub fn outstanding_ids(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.alarm_count {
            if let Some(a) = &self.alarms[i] {
                if a.state.outstanding() {
                    out.push(a.alarm_id);
                }
            }
            i += 1;
        }
        out
    }

    /// 报告行渲染：逐告警"行号|单号|违约天数|级别|状态"。
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== 跟随 SLA 违约告警账 · 四要素审计 ===".to_string());
        let mut i = 0usize;
        while i < self.alarm_count {
            if let Some(a) = &self.alarms[i] {
                let mut line = String::from("A");
                line.push_str(&itoa(a.alarm_id as u64));
                line.push('|');
                line.push_str(&itoa(a.ticket_id as u64));
                line.push_str("|超时");
                line.push_str(&itoa(a.overdue_days as u64));
                line.push_str("|");
                line.push_str(a.level.zh());
                line.push_str("|");
                line.push_str(match a.state {
                    HandleState::Unhandled => "未处置",
                    HandleState::Escalated => "已升级",
                    HandleState::Handled => "已处置",
                });
                lines.push(line);
            }
            i += 1;
        }
        lines
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：超时注入必告警 + 四要素齐 + 未处置拦截收口。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut lg = SlaLedger::new();
    // 三张单：t1 恰 7 日（不违约）、t2 超 8 日、t3 超 12 日。
    let t1 = lg.open_ticket("跟随单·正常", 10)?;
    let t2 = lg.open_ticket("跟随单·中危", 9)?;
    let t3 = lg.open_ticket("跟随单·高危", 5)?;
    let today = 17;
    // 第 7 日：不超时（<= SLA_DAYS），拒绝出告警。
    match lg.scan_ticket(t1, 17) {
        Err(c) if c == E_NO_VIOLATION => {}
        _ => return Err(SEG_BASE),
    }
    let raised = lg.scan_all(today)?;
    if raised.len() != 2 {
        return Err(SEG_BASE);
    }
    // 四要素齐备自证：逐行 valid() 必须全真。
    let mut i = 0usize;
    while i < lg.alarm_len() {
        let row = lg.get_alarm((i + 1) as u32)?;
        if !row.valid() {
            return Err(SEG_BASE);
        }
        i += 1;
    }
    // 守恒：两条告警均为 Unhandled。
    if !lg.conserved() || lg.unhandled_count() != 2 {
        return Err(SEG_BASE);
    }
    // 收口必须被拦。
    if lg.close_out().is_ok() {
        return Err(SEG_BASE);
    }
    // 处置 t1 告警 + 升级后处置 t2 告警 → 收口放行。
    lg.handle(raised[0])?;
    lg.escalate(raised[1])?;
    if lg.close_out().is_ok() {
        return Err(SEG_BASE); // Escalated 仍拦。
    }
    lg.resolve_escalated(raised[1])?;
    let h = lg.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 封账后写拒绝。
    if lg.open_ticket("late", 0).is_ok() {
        return Err(SEG_BASE);
    }
    // 错误路径：重复告警。
    let mut lg2 = SlaLedger::new();
    let x = lg2.open_ticket("dup", 0)?;
    let _ = lg2.scan_ticket(x, 9)?;
    match lg2.scan_ticket(x, 10) {
        Err(c) if c == E_ALARM_DUP => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}

impl SlaLedger {
    /// 未处置计数（演练断言用）。
    pub fn unhandled_count(&self) -> u32 {
        self.unhandled
    }
}
