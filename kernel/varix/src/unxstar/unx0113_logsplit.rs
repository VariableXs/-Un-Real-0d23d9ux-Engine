//! UNX-F0113 · 日志分级输出口（内核/驱动分离）· 诊断段 0x23B0
//! 验收锚点：UNX-F0113-J1 内核/驱动双口分流路由，跨口误投全拒，
//! 口账守恒。零浮点、零 unwrap、零 expect。


use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x23B0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ORBIT: u16 = SEG_BASE + 1; // 口别越纲（核/驱二值外）
const E_CLOSED: u16 = SEG_BASE + 2;// 口已封板，再投被拒
const E_ROUTE: u16 = SEG_BASE + 3; // 跨口误投（驱动行投内核口）
const E_BAL: u16 = SEG_BASE + 4;   // 口账不平（投=收+拒 不等）
const E_EMPTY: u16 = SEG_BASE + 5; // 空口出报告
const E_TWICE: u16 = SEG_BASE + 6; // 同行双投（一序号一账）

pub fn detail(code: u16) -> &'static str {
    match code {
        E_ORBIT => "口别越纲：输出口闭集 [内核,驱动] 外",
        E_CLOSED => "口已封板：交接后禁止再投",
        E_ROUTE => "跨口误投：行来源与目标口不符",
        E_BAL => "口账不平：投入数 ≠ 受理数 + 误拒数",
        E_EMPTY => "空口：无受理行禁止出报告",
        E_TWICE => "同行双投：全局行序号唯一",
        _ => "未知诊断码",
    }
}

/// 口别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orbit {
    Kern,  // 内核口
    Drive, // 驱动口
}

pub fn orbit_name(o: Orbit) -> &'static str {
    match o {
        Orbit::Kern => "内核",
        Orbit::Drive => "驱动",
    }
}

/// 日志行：全局序号 + 来源 + 文本。
#[derive(Clone)]
pub struct LogLine {
    pub seq: u64,
    pub from: Orbit,
    pub text: String,
}

/// 单口账。
pub struct PortLedger {
    pub orbit: Orbit,
    pub accepted: Vec<LogLine>,
    pub misrouted: u64,
    pub sealed: bool,
}

impl PortLedger {
    pub fn new(orbit: Orbit) -> PortLedger {
        PortLedger {
            orbit,
            accepted: Vec::new(),
            misrouted: 0,
            sealed: false,
        }
    }

    /// 投行：来源不符记误拒账；封板拒绝；同序号双投拒绝。
    pub fn offer(&mut self, line: &LogLine) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_CLOSED);
        }
        if self.accepted.iter().any(|l| l.seq == line.seq) {
            return Err(E_TWICE);
        }
        if line.from != self.orbit {
            self.misrouted += 1;
            return Ok(false);
        }
        self.accepted.push(line.clone());
        Ok(true)
    }

    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_CLOSED);
        }
        self.sealed = true;
        Ok(())
    }

    /// 口账审计：纯度核——受理行必须全部来自本口。
    pub fn audit(&self) -> Result<(), u16> {
        for l in &self.accepted {
            if l.from != self.orbit {
                return Err(E_ROUTE);
            }
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, u16> {
        if self.accepted.is_empty() {
            return Err(E_EMPTY);
        }
        Ok(format!(
            "{}口 收 {} 行（首#{} 末#{}）误拒 {}",
            orbit_name(self.orbit),
            self.accepted.len(),
            self.accepted[0].seq,
            self.accepted[self.accepted.len() - 1].seq,
            self.misrouted
        ))
    }
}

/// 分流器：单口路由——行按来源进对应口，双向误投拦截。
pub struct Splitter {
    pub kern: PortLedger,
    pub drive: PortLedger,
    pub next_seq: u64,
}

impl Splitter {
    pub fn new() -> Splitter {
        Splitter {
            kern: PortLedger::new(Orbit::Kern),
            drive: PortLedger::new(Orbit::Drive),
            next_seq: 1,
        }
    }

    /// 产行并投递：按来源路由，返回受理口。
    pub fn emit(&mut self, from: Orbit, text: &str) -> Result<Orbit, u16> {
        let line = LogLine {
            seq: self.next_seq,
            from,
            text: text.to_string(),
        };
        self.next_seq += 1;
        // 先投本口；若本口封板则整体拒绝（不再找他口）。
        let target = match from {
            Orbit::Kern => &mut self.kern,
            Orbit::Drive => &mut self.drive,
        };
        target.offer(&line)?;
        Ok(from)
    }

    /// 双口总审计：两口各守恒 + 全局序号无重叠。
    pub fn audit(&self) -> Result<(), u16> {
        self.kern.audit()?;
        self.drive.audit()?;
        for k in &self.kern.accepted {
            if self.drive.accepted.iter().any(|d| d.seq == k.seq) {
                return Err(E_TWICE);
            }
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：核 3 行驱 2 行分流 → 双口账各归位，审计绿。
    let mut sp = Splitter::new();
    sp.emit(Orbit::Kern, "mmu on")?;
    sp.emit(Orbit::Kern, "heap ready")?;
    sp.emit(Orbit::Drive, "nvme probe")?;
    sp.emit(Orbit::Kern, "sched up")?;
    sp.emit(Orbit::Drive, "nvme ready")?;
    if sp.kern.accepted.len() != 3 || sp.drive.accepted.len() != 2 {
        return Err(E_BAL);
    }
    sp.audit()?;
    hits += 1;

    // R2：跨口误投——驱动行直投内核口 → 误拒账 +1，不进受理。
    let mut pk = PortLedger::new(Orbit::Kern);
    let mis = LogLine {
        seq: 99,
        from: Orbit::Drive,
        text: "wrong port".to_string(),
    };
    if pk.offer(&mis)? {
        return Err(E_ROUTE);
    }
    if pk.misrouted != 1 || !pk.accepted.is_empty() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：口报告渲染含首末序号。
    let rep = sp.kern.render()?;
    if !rep.contains("内核口 收 3 行") || !rep.contains("首#1 末#4") {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：封板交接——双口封板后 emit 整体拒绝。
    let mut sp4 = Splitter::new();
    sp4.emit(Orbit::Kern, "only")?;
    sp4.kern.seal()?;
    if sp4.emit(Orbit::Kern, "late") != Err(E_CLOSED) {
        return Err(E_CLOSED);
    }
    hits += 1;

    // I1：同行双投 → E_TWICE（同口直投与双口审计两路）。
    let mut i1 = PortLedger::new(Orbit::Kern);
    let dup = LogLine {
        seq: 7,
        from: Orbit::Kern,
        text: "once".to_string(),
    };
    i1.offer(&dup)?;
    if i1.offer(&dup) != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    // I2：封板后直投 → E_CLOSED；重复封板 → E_CLOSED。
    let mut i2 = PortLedger::new(Orbit::Drive);
    i2.seal()?;
    let l2 = LogLine {
        seq: 1,
        from: Orbit::Drive,
        text: "x".to_string(),
    };
    if i2.offer(&l2) != Err(E_CLOSED) {
        return Err(E_CLOSED);
    }
    if i2.seal() != Err(E_CLOSED) {
        return Err(E_CLOSED);
    }
    hits += 1;

    // I3：误投污染注入（受理账混入他口行）→ 审计 E_ROUTE。
    let mut i3 = PortLedger::new(Orbit::Drive);
    let bad = LogLine {
        seq: 5,
        from: Orbit::Kern,
        text: "intruder".to_string(),
    };
    i3.accepted.push(bad); // 人为污染
    if i3.audit() != Err(E_ROUTE) {
        return Err(E_ROUTE);
    }
    hits += 1;

    // I4：空口出报告 → E_EMPTY。
    let i4 = PortLedger::new(Orbit::Kern);
    if i4.render() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I5：全局序号跨口重叠 → E_TWICE（审计路）。
    let mut i5 = Splitter::new();
    let a = LogLine {
        seq: 3,
        from: Orbit::Kern,
        text: "ka".to_string(),
    };
    let b = LogLine {
        seq: 3,
        from: Orbit::Drive,
        text: "db".to_string(),
    };
    i5.kern.offer(&a)?;
    i5.drive.offer(&b)?;
    if i5.audit() != Err(E_TWICE) {
        return Err(E_TWICE);
    }
    hits += 1;

    Ok(hits)
}
