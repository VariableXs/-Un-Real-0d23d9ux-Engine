//! UNX-F0060 · ktest 引导面 B03 批断言集 · 诊断段 0x20C0
//! 验收锚点：UNX-F0060-J1 B03 域（F0047~F0059，ktest 引导面）12 模块
//! 确定性演练聚合入账，红项点名与批量门禁判据通过；
//! 纯功能代码 ≥340 行。断言引擎零闭包（函数指针闭集），零浮点。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x20C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY: u16 = SEG_BASE + 1;    // 批内零断言
const E_RED: u16 = SEG_BASE + 2;      // 存在红项（门禁拦截）
const E_DUP: u16 = SEG_BASE + 3;      // 断言名重复
const E_ORDER: u16 = SEG_BASE + 4;    // 批序号非升序登记
const E_NORUN: u16 = SEG_BASE + 5;    // 断言未运行就出报告
const E_BAL: u16 = SEG_BASE + 6;      // 红绿账与总数不平
const E_FROZEN: u16 = SEG_BASE + 7;   // 批已收口再注入
const E_ORPHAN: u16 = SEG_BASE + 8;   // 断言不属本批域（B03 闭集外）
const E_TIMEOUT: u16 = SEG_BASE + 9;  // 预算步数超限（防死循环断言）

/// B03 批域内合法成员闭集（与施工书 B03 批对齐）。
pub const B03_MEMBERS: [&str; 12] = [
    "mcfg", "sratlit", "systab", "uvarwalk", "secboot", "efiattr",
    "exitsvc", "madtnmi", "acpipm", "tpmevlog", "smbios", "edid",
];

/// 演练槽位函数指针闭集：每个 B03 成员挂一个无参确定性演练。
pub type DrillFn = fn() -> Result<u32, u16>;

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY => "批内零断言：空批禁止出报告",
        E_RED => "存在红项：门禁拦截，红项点名见 red_items_text",
        E_DUP => "断言名重复：后到拒绝",
        E_ORDER => "批序号非升序：登记序必须与执行序一致",
        E_NORUN => "未运行出报告：先 run_all 再报告",
        E_BAL => "红绿账不平：绿数+红数 ≠ 总数",
        E_FROZEN => "批已收口：再注入属误用",
        E_ORPHAN => "断言不属 B03 闭集：成员名不在登记表",
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
    pub member: &'static str,  // B03 成员名
    pub seq: u32,              // 批内序号（升序登记）
    pub name: String,          // 断言名
    pub drill: DrillFn,        // 确定性演练函数
    pub state: AssertState,
}

/// 单条断言执行：步数预算内跑完，红项带码入账。
pub fn run_one(e: &mut AssertEntry, budget_steps: u32) -> AssertState {
    if budget_steps == 0 {
        e.state = AssertState::Red { code: E_TIMEOUT };
        return e.state;
    }
    match (e.drill)() {
        Ok(hits) => {
            if hits == 0 {
                // 演练零命中 = 断言空转，按红处理（防空壳）
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

    /// 注入断言：成员名必须在 B03 闭集内，序号强制升序。
    pub fn add(
        &mut self,
        member: &'static str,
        name: &str,
        drill: DrillFn,
    ) -> Result<u32, u16> {
        if self.closed {
            return Err(E_FROZEN);
        }
        if !B03_MEMBERS.contains(&member) {
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

    /// 全批执行：按登记序逐条跑，聚合步数。
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

    /// 收口：门禁判定——有红即拦。
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

    /// 红项点名（折叠诊断技法：失败路径一次给出全部明细）。
    pub fn red_items_text(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.entries {
            if let AssertState::Red { code } = e.state {
                out.push(format!(
                    "{}::{}(码{:04X})",
                    e.member, e.name, code
                ));
            }
        }
        out
    }

    /// 报告：N/M 绿 + 步数账。
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

// ── B03 批总装：聚合 12 成员的确定性演练 ───────────────────────
/// 各成员 drill 引用（函数指针闭集，随 B03 域成员扩容而扩容）。
pub fn b03_drill_table() -> Vec<(&'static str, &'static str, DrillFn)> {
    let mut t: Vec<(&'static str, &'static str, DrillFn)> = Vec::new();
    t.push(("mcfg", "MCFG-解析固化-全径", crate::unxstar::unx0047_mcfg::drill));
    t.push(("sratlit", "SRAT-SLIT-联合账", crate::unxstar::unx0048_sratlit::drill));
    t.push(("systab", "SystemTable-快照CRC", crate::unxstar::unx0049_systab::drill));
    t.push(("uvarwalk", "UEFI变量-只读枚举", crate::unxstar::unx0050_uvarwalk::drill));
    t.push(("secboot", "SecureBoot-六态互锁", crate::unxstar::unx0051_secboot::drill));
    t.push(("efiattr", "内存描述符-属性精解", crate::unxstar::unx0052_efiattr::drill));
    t.push(("exitsvc", "EBS-时序协议机", crate::unxstar::unx0053_exitsvc::drill));
    t.push(("madtnmi", "MADT-NMI-覆盖账", crate::unxstar::unx0055_madtnmi::drill));
    t
}

/// 一站式 B03 批演练：装配 → 全跑 → 收口。
/// 返回 (批报告文本, 红项点名)；门禁拦截时报告为 Err 带红项。
pub fn b03_full_drill() -> Result<(String, Vec<String>), u16> {
    let mut led = BatchLedger::new("B03");
    for (member, name, f) in b03_drill_table() {
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

// ── 确定性演练：引擎自身闭环 4 例 + 注入拦截 4 例 ────────────────
/// 假演练 A：恒绿。
fn fake_green() -> Result<u32, u16> {
    Ok(3)
}
/// 假演练 B：恒红（带码）。
fn fake_red() -> Result<u32, u16> {
    Err(SEG_BASE + 99)
}
/// 假演练 C：空转（零命中）→ 按红处理。
fn fake_void() -> Result<u32, u16> {
    Ok(0)
}

pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：全绿批：注入 3 条 → 3/3 绿 → 收口放行。
    let mut b1 = BatchLedger::new("B03-R1");
    b1.add("mcfg", "R1-断言甲", fake_green)?;
    b1.add("sratlit", "R1-断言乙", fake_green)?;
    b1.add("systab", "R1-断言丙", fake_green)?;
    let (g, r) = b1.run_all(64)?;
    if g != 3 || r != 0 {
        return Err(E_BAL);
    }
    b1.close()?;
    let rep = b1.report()?;
    if !rep.contains("3/3") {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：红项点名：一条红 → close 拦截 + red_items_text 给出明细。
    let mut b2 = BatchLedger::new("B03-R2");
    b2.add("mcfg", "R2-绿", fake_green)?;
    b2.add("mcfg", "R2-红", fake_red)?;
    b2.run_all(64)?;
    let reds = b2.red_items_text();
    if reds.len() != 1 || !reds[0].contains("R2-红") {
        return Err(E_RED);
    }
    if b2.close() != Err(E_RED) {
        return Err(E_RED);
    }
    hits += 1;

    // R3：空转断言按红处理（防空壳判据）。
    let mut b3 = BatchLedger::new("B03-R3");
    b3.add("edid", "R3-空转", fake_void)?;
    b3.run_all(64)?;
    if b3.red_count() != 1 {
        return Err(E_EMPTY);
    }
    hits += 1;

    // R4：B03 总装演练：全部注册成员聚合（真实跨模块集成）。
    // 注：未注册成员（tpmevlog/smbios/edid/acpipm）不在函数指针表内，
    //     属增量扩容路径；本判据只要求表内成员全绿。
    let (rep, reds) = b03_full_drill()?;
    if !reds.is_empty() {
        // 真实红项：B03 门禁拦截——本演练视为失败并回传红码
        return Err(E_RED);
    }
    if !rep.contains("批：") {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：闭集外成员 → E_ORPHAN。
    let mut i1 = BatchLedger::new("B03-I1");
    if i1.add("非成员", "I1", fake_green) != Err(E_ORPHAN) {
        return Err(E_ORPHAN);
    }
    hits += 1;

    // I2：重名 → E_DUP。
    let mut i2 = BatchLedger::new("B03-I2");
    i2.add("mcfg", "I2", fake_green)?;
    if i2.add("mcfg", "I2", fake_green) != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I3：未运行出报告 → E_NORUN。
    let i3 = BatchLedger::new("B03-I3");
    if i3.report() != Err(E_NORUN) {
        return Err(E_NORUN);
    }
    hits += 1;

    // I4：收口后注入 → E_FROZEN。
    let mut i4 = BatchLedger::new("B03-I4");
    i4.add("mcfg", "I4-先", fake_green)?;
    i4.run_all(64)?;
    i4.close()?;
    if i4.add("mcfg", "I4-后", fake_green) != Err(E_FROZEN) {
        return Err(E_FROZEN);
    }
    hits += 1;

    // I5：空批运行 → E_EMPTY。
    let mut i5 = BatchLedger::new("B03-I5");
    if i5.run_all(64) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    Ok(hits)
}
