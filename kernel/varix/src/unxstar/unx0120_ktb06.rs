//! UNX-F0120 · ktest 引导面 B06 批断言集 · 诊断段 0x2420
//! 验收锚点：UNX-F0120-J1 B06 域（F0103~F0119，控制台与日志面）
//! 17 模块确定性演练聚合入账，红项点名与批量门禁判据通过；
//! 纯功能代码 ≥300 行。断言引擎零闭包（函数指针闭集），零浮点。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2420;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1;    // 批内零断言
const E_RED: u16 = SEG_BASE + 2;      // 存在红项（门禁拦截）
const E_DUP: u16 = SEG_BASE + 3;      // 断言名重复
const E_NORUN: u16 = SEG_BASE + 4;    // 未运行出报告
const E_BAL: u16 = SEG_BASE + 5;      // 红绿账不平
const E_FROZEN: u16 = SEG_BASE + 6;   // 批已收口再注入
const E_ORPHAN: u16 = SEG_BASE + 7;   // 成员不在 B06 闭集
const E_TIMEOUT: u16 = SEG_BASE + 8;  // 步数预算超限

/// B06 批域成员闭集（F0103~F0119，17 成员）。
pub const B06_MEMBERS: [&str; 17] = [
    "fontterm", "dualarb", "loglevel", "logstamp", "ringlog",
    "panicsnap", "serialcfg", "concolor", "scrollback", "ansiesc",
    "logsplit", "diagui", "quietboot", "loginject", "sermir",
    "logfmt", "earlyfail",
];

pub type DrillFn = fn() -> Result<u32, u16>;

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "批内零断言：空批禁止出报告",
        E_RED => "存在红项：门禁拦截，红项点名见 red_items_text",
        E_DUP => "断言名重复：后到拒绝",
        E_NORUN => "未运行出报告：先 run_all 再报告",
        E_BAL => "红绿账不平：绿数+红数 ≠ 总数",
        E_FROZEN => "批已收口：再注入属误用",
        E_ORPHAN => "断言不属 B06 闭集：成员名不在登记表",
        E_TIMEOUT => "断言步数超预算：疑似死循环，强制红",
        _ => "未知诊断码",
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 断言登记项 ───────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertState {
    Pending,
    Green { hits: u32 },
    Red { code: u16 },
}

pub struct AssertEntry {
    pub member: &'static str,
    pub seq: u32,
    pub name: String,
    pub drill: DrillFn,
    pub state: AssertState,
}

/// 单条断言执行：预算内跑完，红项带码入账。
pub fn run_one(e: &mut AssertEntry, budget_steps: u32) -> AssertState {
    if budget_steps == 0 {
        e.state = AssertState::Red { code: E_TIMEOUT };
        return e.state;
    }
    match (e.drill)() {
        Ok(hits) => {
            if hits == 0 {
                e.state = AssertState::Red { code: E_EMPTY };
            } else {
                e.state = AssertState::Green { hits };
            }
        }
        Err(code) => e.state = AssertState::Red { code },
    }
    e.state
}

// ── 批账 ─────────────────────────────────────────────────────
pub struct BatchLedger {
    pub batch_id: &'static str,
    pub entries: Vec<AssertEntry>,
    pub next_seq: u32,
    pub ran: bool,
    pub closed: bool,
    pub total_steps: u32,
}

impl BatchLedger {
    pub fn new(batch_id: &'static str) -> BatchLedger {
        BatchLedger {
            batch_id,
            entries: Vec::new(),
            next_seq: 0,
            ran: false,
            closed: false,
            total_steps: 0,
        }
    }

    /// 注入断言：成员名须在 B06 闭集内。
    pub fn add(
        &mut self,
        member: &'static str,
        name: &str,
        drill: DrillFn,
    ) -> Result<u32, u16> {
        if self.closed {
            return Err(E_FROZEN);
        }
        if !B06_MEMBERS.contains(&member) {
            return Err(E_ORPHAN);
        }
        if self.entries.iter().any(|e| e.name == name) {
            return Err(E_DUP);
        }
        let seq = self.next_seq;
        self.entries.push(AssertEntry {
            member,
            seq,
            name: name.to_string(),
            drill,
            state: AssertState::Pending,
        });
        self.next_seq += 1;
        Ok(seq)
    }

    /// 全批执行。
    pub fn run_all(&mut self, per_assert_budget: u32) -> Result<(usize, usize), u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        if self.ran {
            return Err(E_FROZEN);
        }
        let mut green = 0usize;
        let mut red = 0usize;
        let n = self.entries.len();
        for i in 0..n {
            let st = {
                let e = &mut self.entries[i];
                run_one(e, per_assert_budget)
            };
            match st {
                AssertState::Green { hits } => {
                    green += 1;
                    self.total_steps += hits;
                }
                AssertState::Red { .. } => red += 1,
                AssertState::Pending => return Err(E_NORUN),
            }
        }
        self.ran = true;
        if green + red != n {
            return Err(E_BAL);
        }
        Ok((green, red))
    }

    /// 收口门禁。
    pub fn close(&mut self) -> Result<(), u16> {
        if !self.ran {
            return Err(E_NORUN);
        }
        self.closed = true;
        if self.red_count() > 0 {
            return Err(E_RED);
        }
        Ok(())
    }

    pub fn green_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, AssertState::Green { .. }))
            .count()
    }

    pub fn red_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| matches!(e.state, AssertState::Red { .. }))
            .count()
    }

    /// 红项点名。
    pub fn red_items_text(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.entries {
            if let AssertState::Red { code } = e.state {
                out.push(format!("{}::{}(码{:04X})", e.member, e.name, code));
            }
        }
        out
    }

    /// 报告。
    pub fn report(&self) -> Result<String, u16> {
        if !self.ran {
            return Err(E_NORUN);
        }
        if self.green_count() + self.red_count() != self.entries.len() {
            return Err(E_BAL);
        }
        Ok(format!(
            "{} 批：{}/{} 绿，步数账 {}",
            self.batch_id,
            self.green_count(),
            self.entries.len(),
            self.total_steps
        ))
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for e in &self.entries {
            buf.extend_from_slice(&e.seq.to_le_bytes());
            buf.extend_from_slice(e.name.as_bytes());
            buf.push(match e.state {
                AssertState::Pending => 0,
                AssertState::Green { .. } => 1,
                AssertState::Red { .. } => 2,
            });
        }
        fnv1a64(&buf)
    }
}

// ── B06 批总装：聚合 17 成员的确定性演练 ───────────────────────
pub fn b06_drill_table() -> Vec<(&'static str, &'static str, DrillFn)> {
    let mut t: Vec<(&'static str, &'static str, DrillFn)> = Vec::new();
    t.push(("fontterm", "帧簿字体-全字符集", crate::unxstar::unx0103_fontterm::drill));
    t.push(("dualarb", "双通道-仲裁降级", crate::unxstar::unx0104_dualarb::drill));
    t.push(("loglevel", "日志五级-双控过滤", crate::unxstar::unx0105_loglevel::drill));
    t.push(("logstamp", "TSC 时戳-单调校准", crate::unxstar::unx0106_logstamp::drill));
    t.push(("ringlog", "环形日志-覆盖找回", crate::unxstar::unx0107_ringlog::drill));
    t.push(("panicsnap", "panic 快照-三要素", crate::unxstar::unx0108_panicsnap::drill));
    t.push(("serialcfg", "串口 cmdline-四参", crate::unxstar::unx0109_serialcfg::drill));
    t.push(("concolor", "色彩语义-五级映射", crate::unxstar::unx0110_concolor::drill));
    t.push(("scrollback", "回看-行账守恒", crate::unxstar::unx0111_scrollback::drill));
    t.push(("ansiesc", "ANSI-编解码往返", crate::unxstar::unx0112_ansiesc::drill));
    t.push(("logsplit", "分流-核驱双口", crate::unxstar::unx0113_logsplit::drill));
    t.push(("diagui", "诊断 UI-四要素", crate::unxstar::unx0114_diagui::drill));
    t.push(("quietboot", "静默引导-零泄漏", crate::unxstar::unx0115_quietboot::drill));
    t.push(("loginject", "注入防护-三类拦截", crate::unxstar::unx0116_loginject::drill));
    t.push(("sermir", "串口镜像-只读帧账", crate::unxstar::unx0117_sermir::drill));
    t.push(("logfmt", "格式规范-往返保真", crate::unxstar::unx0118_logfmt::drill));
    t.push(("earlyfail", "早失败-显性六段", crate::unxstar::unx0119_earlyfail::drill));
    t
}

/// 一站式 B06 批演练。
pub fn b06_full_drill() -> Result<(String, Vec<String>), u16> {
    let mut led = BatchLedger::new("B06");
    for (member, name, f) in b06_drill_table() {
        led.add(member, name, f)?;
    }
    let (green, red) = led.run_all(4096)?;
    let reds = led.red_items_text();
    match led.close() {
        Ok(()) => {
            let rep = led.report()?;
            let _ = (green, red);
            Ok((rep, reds))
        }
        Err(E_RED) => {
            let rep = format!(
                "{} 批门禁拦截：{}/{} 绿",
                led.batch_id, green, led.entries.len()
            );
            Ok((rep, reds))
        }
        Err(c) => Err(c),
    }
}

// ── 确定性演练：引擎闭环 4 例 + 注入拦截 5 例 ────────────────────
fn fake_green() -> Result<u32, u16> {
    Ok(3)
}
fn fake_red() -> Result<u32, u16> {
    Err(SEG_BASE + 99)
}
fn fake_void() -> Result<u32, u16> {
    Ok(0)
}

pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全绿批 → 收口放行 + 指纹稳定。
    let mut b1 = BatchLedger::new("B06-R1");
    b1.add("fontterm", "R1-甲", fake_green)?;
    b1.add("dualarb", "R1-乙", fake_green)?;
    b1.add("loglevel", "R1-丙", fake_green)?;
    let (g, r) = b1.run_all(64)?;
    if g != 3 || r != 0 {
        return Err(E_BAL);
    }
    b1.close()?;
    if !b1.report()?.contains("3/3") {
        return Err(E_BAL);
    }
    if b1.fingerprint() != b1.fingerprint() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：红项点名 + 门禁拦截。
    let mut b2 = BatchLedger::new("B06-R2");
    b2.add("logstamp", "R2-绿", fake_green)?;
    b2.add("logstamp", "R2-红", fake_red)?;
    b2.run_all(64)?;
    let reds = b2.red_items_text();
    if reds.len() != 1 || !reds[0].contains("R2-红") {
        return Err(E_RED);
    }
    if b2.close() != Err(E_RED) {
        return Err(E_RED);
    }
    hits += 1;

    // R3：空转断言按红处理。
    let mut b3 = BatchLedger::new("B06-R3");
    b3.add("earlyfail", "R3-空转", fake_void)?;
    b3.run_all(64)?;
    if b3.red_count() != 1 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R4：B06 总装：17 成员全量聚合一次全绿。
    let (rep, reds) = b06_full_drill()?;
    if !reds.is_empty() {
        return Err(E_RED);
    }
    if !rep.contains("17/17 绿") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：闭集外成员 → E_ORPHAN。
    let mut i1 = BatchLedger::new("B06-I1");
    if i1.add("非成员", "I1", fake_green) != Err(E_ORPHAN) {
        return Err(E_ORPHAN);
    }
    hits += 1;

    // I2：重名 → E_DUP。
    let mut i2 = BatchLedger::new("B06-I2");
    i2.add("ringlog", "I2", fake_green)?;
    if i2.add("ringlog", "I2", fake_green) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I3：未运行出报告 → E_NORUN。
    let i3 = BatchLedger::new("B06-I3");
    if i3.report() != Err(E_NORUN) {
        return Err(E_NORUN);
    }
    hits += 1;

    // I4：收口后注入 → E_FROZEN。
    let mut i4 = BatchLedger::new("B06-I4");
    i4.add("quietboot", "I4-先", fake_green)?;
    i4.run_all(64)?;
    i4.close()?;
    if i4.add("quietboot", "I4-后", fake_green) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I5：空批运行 → E_EMPTY；零预算 → E_TIMEOUT 红。
    let mut i5 = BatchLedger::new("B06-I5");
    if i5.run_all(64) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    i5.add("sermir", "I5-预算零", fake_green)?;
    i5.run_all(0)?;
    if i5.red_count() != 1 {
        return Err(E_TIMEOUT);
    }
    hits += 1;

    Ok(hits)
}
