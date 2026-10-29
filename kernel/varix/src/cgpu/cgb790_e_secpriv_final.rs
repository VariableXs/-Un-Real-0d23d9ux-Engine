//! CGPU-F0790 · E 域安全与隐私终审（目标 340 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥340 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0790，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域安全与隐私终审——把安全控制项与隐私控制项合并为
//!   一张终审清单，逐项给出证据哈希（整数摘要），闭集风险评级，
//!   高危发现清零才可宣通过；发现账本定容守恒。
//!
//! 设计纪律：
//! - 控制项闭集两类：Security（安全）/ Privacy（隐私），各带
//!   控制强度闭集三档：Basic / Hardened / Verified。
//! - 发现（Finding）风险闭集四级：Info / Low / High / Critical；
//!   终审通过条件：High+Critical==0，且控制项全部有证据摘要。
//! - 证据摘要：FNV-1a 64 位整数哈希（确定性、无外部依赖），
//!   证据为空串即口径非法。
//! - 发现账本定容：FIND_CAP=80，超容逐出最旧并计数，
//!   守恒恒等式：total_findings == held + evicted。
//! - 整改状态机：Open→Triaged→Mitigated→Closed，跳步即 Err。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6E00 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6E00，批内 CGPU-F0790 独占）。
pub const SEG_BASE: u16 = 0x6E00;

/// 控制项编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 发现账本已满。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 证据摘要口径非法（空证据）。
pub const E_EVID: u16 = SEG_BASE + 3;
/// 未知控制项引用。
pub const E_NOCTL: u16 = SEG_BASE + 4;
/// 整改状态机跳步。
pub const E_STATE: u16 = SEG_BASE + 5;
/// 终审存在高危发现，不通过。
pub const E_RISK: u16 = SEG_BASE + 6;

/// 发现账本容量。
pub const FIND_CAP: usize = 80;

/// FNV-1a 64 位整数摘要（确定性）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 控制项类别（闭集两类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtlDomain {
    /// 安全控制。
    Security,
    /// 隐私控制。
    Privacy,
}

impl CtlDomain {
    pub fn ord(self) -> u8 {
        match self {
            CtlDomain::Security => 0,
            CtlDomain::Privacy => 1,
        }
    }
}

/// 控制强度（闭集三档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CtlStrength {
    Basic,
    Hardened,
    Verified,
}

/// 控制项。
#[derive(Clone, Debug)]
pub struct Control {
    /// 编号（清单内唯一）。
    pub id: u16,
    /// 名称。
    pub name: String,
    /// 类别。
    pub domain: CtlDomain,
    /// 强度。
    pub strength: CtlStrength,
    /// 证据摘要（0 = 未提供）。
    pub evid_hash: u64,
}

impl Control {
    /// 登记时必须附证据。
    pub fn new(id: u16, name: &str, domain: CtlDomain, strength: CtlStrength, evid: &str) -> Result<Self, u16> {
        if evid.is_empty() {
            return Err(E_EVID);
        }
        Ok(Control {
            id,
            name: name.to_string(),
            domain,
            strength,
            evid_hash: fnv1a64(evid.as_bytes()),
        })
    }
}

/// 发现风险（闭集四级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Risk {
    Info,
    Low,
    High,
    Critical,
}

impl Risk {
    pub fn ord(self) -> u8 {
        match self {
            Risk::Info => 0,
            Risk::Low => 1,
            Risk::High => 2,
            Risk::Critical => 3,
        }
    }
}

/// 整改状态（闭集四态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixState {
    Open,
    Triaged,
    Mitigated,
    Closed,
}

impl FixState {
    /// 合法前进一格。
    pub fn next(self) -> Option<FixState> {
        match self {
            FixState::Open => Some(FixState::Triaged),
            FixState::Triaged => Some(FixState::Mitigated),
            FixState::Mitigated => Some(FixState::Closed),
            FixState::Closed => None,
        }
    }
    pub fn ord(self) -> u8 {
        match self {
            FixState::Open => 0,
            FixState::Triaged => 1,
            FixState::Mitigated => 2,
            FixState::Closed => 3,
        }
    }
}

/// 发现记录。
#[derive(Clone, Debug)]
pub struct Finding {
    /// 引用的控制项编号。
    pub ctl_id: u16,
    /// 风险级。
    pub risk: Risk,
    /// 整改状态。
    pub state: FixState,
    /// 描述摘要哈希（脱敏：原文不入账）。
    pub desc_hash: u64,
}

/// 发现账本（环形定容）。
#[derive(Clone, Debug)]
pub struct FindingLedger {
    ring: Vec<Finding>,
    evicted: u64,
    total: u64,
}

impl FindingLedger {
    pub fn new() -> Self {
        FindingLedger {
            ring: Vec::new(),
            evicted: 0,
            total: 0,
        }
    }
    pub fn push(&mut self, f: Finding) -> Result<(), u16> {
        if f.desc_hash == 0 {
            return Err(E_EVID);
        }
        if self.ring.len() >= FIND_CAP {
            self.ring.remove(0);
            self.evicted += 1;
        }
        self.ring.push(f);
        self.total += 1;
        Ok(())
    }
    /// 整改推进：找到第一条 Open 且属于 ctl_id 的发现，前进一格。
    pub fn advance(&mut self, ctl_id: u16) -> Result<FixState, u16> {
        for f in self.ring.iter_mut() {
            if f.ctl_id == ctl_id && f.state != FixState::Closed {
                let n = f.state.next().ok_or(E_STATE)?;
                f.state = n;
                return Ok(n);
            }
        }
        Err(E_NOCTL)
    }
    /// 守恒审计：total == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.ring.len() as u64 + self.evicted {
            return Err(E_CAP);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    /// 风险计数（四级闭集）。
    pub fn risk_tally(&self) -> [u32; 4] {
        let mut t = [0u32; 4];
        for f in &self.ring {
            t[f.risk.ord() as usize] += 1;
        }
        t
    }
}

/// 终审：控制项全部有证据摘要 且 High/Critical 未闭环数为 0。
pub fn final_review(controls: &[Control], led: &FindingLedger) -> Result<u32, u16> {
    if controls.is_empty() {
        return Err(E_NOCTL);
    }
    // 控制项编号互异且证据非零。
    for (i, c) in controls.iter().enumerate() {
        if c.evid_hash == 0 {
            return Err(E_EVID);
        }
        for o in controls.iter().skip(i + 1) {
            if o.id == c.id {
                return Err(E_DUP);
            }
        }
        // 每个控制项必须在发现账本中有至少一条记录（审过而非没审）。
        if !led.ring.iter().any(|f| f.ctl_id == c.id) {
            return Err(E_NOCTL);
        }
    }
    // 未闭环高危计数。
    let mut open_high = 0u32;
    for f in &led.ring {
        let unclosed = f.state != FixState::Closed;
        if unclosed && (f.risk == Risk::High || f.risk == Risk::Critical) {
            open_high += 1;
        }
    }
    if open_high > 0 {
        return Err(E_RISK);
    }
    Ok(0)
}

/// 自例程：主路径（全闭环通过）+ 高危未闭环拒绝 + 错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let c1 = Control::new(1, "mem_iso", CtlDomain::Security, CtlStrength::Verified, "evid-1")?;
    let c2 = Control::new(2, "pii_mask", CtlDomain::Privacy, CtlStrength::Hardened, "evid-2")?;
    // 同证据 → 同摘要（确定性核验）。
    let c1b = Control::new(3, "dup_evid", CtlDomain::Security, CtlStrength::Basic, "evid-1")?;
    if c1.evid_hash != c1b.evid_hash {
        return Err(E_EVID);
    }
    // 错误路径 1：空证据。
    if Control::new(9, "x", CtlDomain::Security, CtlStrength::Basic, "") != Err(E_EVID) {
        return Err(E_EVID);
    }
    // 主路径 1：两条低危发现，全部推进到 Closed → 通过。
    let mut led = FindingLedger::new();
    led.push(Finding {
        ctl_id: 1,
        risk: Risk::Low,
        state: FixState::Open,
        desc_hash: fnv1a64(b"f1"),
    })?;
    led.push(Finding {
        ctl_id: 2,
        risk: Risk::Info,
        state: FixState::Open,
        desc_hash: fnv1a64(b"f2"),
    })?;
    // 状态机逐步推进：Open→Triaged→Mitigated→Closed。
    if led.advance(1)? != FixState::Triaged {
        return Err(E_STATE);
    }
    if led.advance(1)? != FixState::Mitigated {
        return Err(E_STATE);
    }
    if led.advance(1)? != FixState::Closed {
        return Err(E_STATE);
    }
    // Closed 再推进 → 跳过它找下一条或报错；此控制项只剩 Closed → E_NOCTL。
    if led.advance(1) != Err(E_NOCTL) {
        return Err(E_STATE);
    }
    // 闭环第二条。
    led.advance(2)?;
    led.advance(2)?;
    led.advance(2)?;
    led.audit()?;
    let t = led.risk_tally();
    if t[Risk::Low.ord() as usize] != 1 || t[Risk::Info.ord() as usize] != 1 {
        return Err(E_RISK);
    }
    let ctrls = [c1, c2];
    final_review(&ctrls, &led)?;
    // 主路径 2：High 未闭环 → 拒绝。
    let c3 = Control::new(4, "sig_check", CtlDomain::Security, CtlStrength::Basic, "evid-3")?;
    let mut led2 = FindingLedger::new();
    led2.push(Finding {
        ctl_id: 4,
        risk: Risk::High,
        state: FixState::Open,
        desc_hash: fnv1a64(b"f3"),
    })?;
    let ctrls2 = [c3];
    if final_review(&ctrls2, &led2) != Err(E_RISK) {
        return Err(E_RISK);
    }
    // High 闭环后 → 通过。
    led2.advance(4)?;
    led2.advance(4)?;
    led2.advance(4)?;
    final_review(&ctrls2, &led2)?;
    // 错误路径 2：控制项缺发现记录（没审过）。
    let c4 = Control::new(5, "unaudited", CtlDomain::Privacy, CtlStrength::Basic, "evid-4")?;
    let ctrls3 = [c1b, c4];
    if final_review(&ctrls3, &led) != Err(E_NOCTL) {
        return Err(E_NOCTL);
    }
    // 错误路径 3：编号重复。
    let c5 = Control::new(1, "again", CtlDomain::Security, CtlStrength::Basic, "evid-5")?;
    let ctrls4 = [c5, c5.clone()];
    if final_review(&ctrls4, &led) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 4：空控制项册。
    if final_review(&[], &led) != Err(E_NOCTL) {
        return Err(E_NOCTL);
    }
    // 错误路径 5：零摘要发现拒绝入账。
    if led2.push(Finding {
        ctl_id: 1,
        risk: Risk::Info,
        state: FixState::Open,
        desc_hash: 0,
    }) != Err(E_EVID)
    {
        return Err(E_EVID);
    }
    // 定容守恒：灌 84 条进 80 容账本，恒等式仍成立。
    let mut led3 = FindingLedger::new();
    for i in 0..84u64 {
        led3.push(Finding {
            ctl_id: 1,
            risk: Risk::Info,
            state: FixState::Closed,
            desc_hash: i + 1,
        })?;
    }
    led3.audit()?;
    if led3.held() != FIND_CAP || led3.evicted != 4 {
        return Err(E_CAP);
    }
    Ok(0)
}
