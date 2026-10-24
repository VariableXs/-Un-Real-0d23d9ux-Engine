//! UNX-F0080 · ktest 引导面 B04 批断言集 · 诊断段 0x21F0
//! 验收锚点：UNX-F0080-J1 B04 域（F0062~F0079，内存图域）10 模块
//! 确定性演练聚合入账，红项点名与批量门禁判据通过；
//! 纯功能代码 ≥330 行。断言引擎零闭包（函数指针闭集），零浮点。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x21F0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1;
const E_RED: u16 = SEG_BASE + 2;
const E_DUP: u16 = SEG_BASE + 3;
const E_ORDER: u16 = SEG_BASE + 4;
const E_NORUN: u16 = SEG_BASE + 5;
const E_BAL: u16 = SEG_BASE + 6;
const E_FROZEN: u16 = SEG_BASE + 7;
const E_ORPHAN: u16 = SEG_BASE + 8;
const E_TIMEOUT: u16 = SEG_BASE + 9;

/// B04 批域内合法成员闭集。
pub const B04_MEMBERS: [&str; 10] = [
    "acpirecl", "low1map", "e820bridge", "memsnap", "alignzone",
    "hugepage", "hotplugbmp", "efimapdup", "earlyalloc", "pageaudit",
];

pub type DrillFn = fn() -> Result<u32, u16>;

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "批内零断言或演练零命中：空转按红处理",
        E_RED => "存在红项：门禁拦截",
        E_DUP => "断言名重复：后到拒绝",
        E_ORDER => "批序号非升序或重跑",
        E_NORUN => "未运行出报告",
        E_BAL => "红绿账不平",
        E_FROZEN => "批已收口：再注入属误用",
        E_ORPHAN => "成员名不在 B04 闭集",
        E_TIMEOUT => "断言步数超预算",
        _ => "未知诊断码",
    }
}

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

// ── B04 批账 ─────────────────────────────────────────────────
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

    pub fn add(&mut self, member: &'static str, name: &str, drill: DrillFn) -> Result<u32, u16> {
        if self.closed {
            return Err(E_FROZEN);
        }
        if !B04_MEMBERS.contains(&member) {
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

    pub fn run_all(&mut self, per_assert_budget: u32) -> Result<(usize, usize), u16> {
        if self.entries.is_empty() {
            return Err(E_EMPTY);
        }
        if self.ran {
            return Err(E_ORDER);
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

    pub fn red_items_text(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|e| match e.state {
                AssertState::Red { code } => {
                    Some(format!("{}::{}(码{:04X})", e.member, e.name, code))
                }
                _ => None,
            })
            .collect()
    }

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

// ── B04 成员 drill 引用表（随域扩容增行）──────────────────────
pub fn b04_drill_table() -> Vec<(&'static str, &'static str, DrillFn)> {
    let mut t: Vec<(&'static str, &'static str, DrillFn)> = Vec::new();
    t.push(("acpirecl", "ACPI-回收窗口-状态机", crate::unxstar::unx0063_acpirecl::drill));
    t.push(("low1map", "低1MB-五区布局", crate::unxstar::unx0064_low1map::drill));
    t.push(("e820bridge", "E820-桥接映射", crate::unxstar::unx0065_e820bridge::drill));
    t.push(("memsnap", "内存图-快照往返", crate::unxstar::unx0066_memsnap::drill));
    t.push(("alignzone", "可用区-对齐裁剪", crate::unxstar::unx0067_alignzone::drill));
    t.push(("hugepage", "大页-分布预判", crate::unxstar::unx0068_hugepage::drill));
    t.push(("hotplugbmp", "热插拔-位图预留", crate::unxstar::unx0069_hotplugbmp::drill));
    t.push(("efimapdup", "MemoryMap-双副本", crate::unxstar::unx0070_efimapdup::drill));
    t.push(("earlyalloc", "早年分配-bump账", crate::unxstar::unx0071_earlyalloc::drill));
    t.push(("pageaudit", "页账本-四账守恒", crate::unxstar::unx0072_pageaudit::drill));
    t
}

/// 一站式 B04 批演练：装配 → 全跑 → 收口。
pub fn b04_full_drill() -> Result<(String, Vec<String>), u16> {
    let mut led = BatchLedger::new("B04");
    for (member, name, f) in b04_drill_table() {
        led.add(member, name, f)?;
    }
    let (green, red) = led.run_all(8192)?;
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
    Ok(5)
}
fn fake_red() -> Result<u32, u16> {
    Err(SEG_BASE + 99)
}
fn fake_void() -> Result<u32, u16> {
    Ok(0)
}

pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全绿批收口放行。
    let mut b1 = BatchLedger::new("B04-R1");
    b1.add("memsnap", "R1-甲", fake_green)?;
    b1.add("alignzone", "R1-乙", fake_green)?;
    b1.add("pageaudit", "R1-丙", fake_green)?;
    let (g, r) = b1.run_all(64)?;
    if g != 3 || r != 0 {
        return Err(E_BAL);
    }
    b1.close()?;
    if !b1.report()?.contains("3/3") {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：红项点名与门禁拦截。
    let mut b2 = BatchLedger::new("B04-R2");
    b2.add("memsnap", "R2-绿", fake_green)?;
    b2.add("efimapdup", "R2-红", fake_red)?;
    b2.run_all(64)?;
    let reds = b2.red_items_text();
    if reds.len() != 1 || !reds[0].contains("R2-红") {
        return Err(E_RED);
    }
    if b2.close() != Err(E_RED) {
        return Err(E_RED);
    }
    hits += 1;

    // R3：B04 总装演练（真实跨模块集成：10 成员全绿）。
    let (rep, reds3) = b04_full_drill()?;
    if !reds3.is_empty() {
        return Err(E_RED);
    }
    if !rep.contains("10/10") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：空转断言按红。
    let mut b4 = BatchLedger::new("B04-R4");
    b4.add("earlyalloc", "R4-空转", fake_void)?;
    b4.run_all(64)?;
    if b4.red_count() != 1 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I1：闭集外成员 → E_ORPHAN。
    let mut i1 = BatchLedger::new("B04-I1");
    if i1.add("B03域", "I1", fake_green) != Err(E_ORPHAN) {
        return Err(E_ORPHAN);
    }
    hits += 1;

    // I2：重名 → E_DUP。
    let mut i2 = BatchLedger::new("B04-I2");
    i2.add("memsnap", "I2", fake_green)?;
    if i2.add("memsnap", "I2", fake_green) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I3：未运行出报告 → E_NORUN。
    let i3 = BatchLedger::new("B04-I3");
    if i3.report() != Err(E_NORUN) {
        return Err(E_NORUN);
    }
    hits += 1;

    // I4：收口后注入 → E_FROZEN。
    let mut i4 = BatchLedger::new("B04-I4");
    i4.add("memsnap", "I4", fake_green)?;
    i4.run_all(64)?;
    i4.close()?;
    if i4.add("memsnap", "I4-后", fake_green) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I5：重跑 → E_ORDER。
    let mut i5 = BatchLedger::new("B04-I5");
    i5.add("memsnap", "I5", fake_green)?;
    i5.run_all(64)?;
    if i5.run_all(64) != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // I6：空批运行 → E_EMPTY。
    let mut i6 = BatchLedger::new("B04-I6");
    if i6.run_all(64) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
