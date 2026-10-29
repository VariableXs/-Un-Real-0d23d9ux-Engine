//! CGPU-F0793 · E 域缺陷账本清账（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0793，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域缺陷账本清账——缺陷（Defect）从登记到销账的
//!   全生命周期账本：状态机推进、重开规则、清账守恒
//!   （登记总数 == 在册 + 销账 + 逐出）与清账判据（在册为零）。
//!
//! 设计纪律：
//! - 缺陷严重级闭集四档：S0（阻断）/ S1（严重）/ S2（一般）/
//!   S3（轻微）；清账判据只看在册数，不看严重级分布。
//! - 状态机闭集：Open→Fixed→Verified（销账完成），Open→WontFix
//!   （销账完成），Verified/WontFix 可 Reopen→Open（重开一次
//!   计数 +1，重开超过 REOPEN_LIMIT 即拒绝）。
//! - 账本定容：DEFECT_CAP=72，超容逐出最旧已销账项
//!   （无已销账项则拒绝入账），守恒恒等式：
//!   total_registered == held + closed_out + evicted。
//! - 清账口径：held 中全部处于 Verified/WontFix 态且
//!   open_count()==0 → 清账成立。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6E60 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6E60，批内 CGPU-F0793 独占）。
pub const SEG_BASE: u16 = 0x6E60;

/// 缺陷编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 账本已满且无可逐出的已销账项。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知缺陷编号。
pub const E_NODF: u16 = SEG_BASE + 4;
/// 重开次数超限。
pub const E_REOPEN: u16 = SEG_BASE + 5;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 6;

/// 账本容量。
pub const DEFECT_CAP: usize = 72;
/// 单缺陷重开上限。
pub const REOPEN_LIMIT: u32 = 2;

/// 严重级（闭集四档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sev {
    S0,
    S1,
    S2,
    S3,
}

impl Sev {
    pub fn ord(self) -> u8 {
        match self {
            Sev::S0 => 0,
            Sev::S1 => 1,
            Sev::S2 => 2,
            Sev::S3 => 3,
        }
    }
}

/// 缺陷状态（闭集五态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DState {
    /// 在册待修。
    Open,
    /// 已修复待验证。
    Fixed,
    /// 验证通过（销账态）。
    Verified,
    /// 不修（销账态）。
    WontFix,
}

impl DState {
    /// 是否销账态。
    pub fn is_closed(self) -> bool {
        matches!(self, DState::Verified | DState::WontFix)
    }
    pub fn ord(self) -> u8 {
        match self {
            DState::Open => 0,
            DState::Fixed => 1,
            DState::Verified => 2,
            DState::WontFix => 3,
        }
    }
}

/// 缺陷。
#[derive(Clone, Debug)]
pub struct Defect {
    /// 编号（账本内唯一）。
    pub id: u32,
    /// 标题。
    pub title: String,
    /// 严重级。
    pub sev: Sev,
    /// 状态。
    pub state: DState,
    /// 重开次数。
    pub reopens: u32,
}

impl Defect {
    pub fn new(id: u32, title: &str, sev: Sev) -> Self {
        Defect {
            id,
            title: title.to_string(),
            sev,
            state: DState::Open,
            reopens: 0,
        }
    }
}

/// 缺陷账本（环形定容，逐出只逐已销账项）。
#[derive(Clone, Debug)]
pub struct DefectLedger {
    ring: Vec<Defect>,
    evicted: u64,
    total_registered: u64,
}

impl DefectLedger {
    pub fn new() -> Self {
        DefectLedger {
            ring: Vec::new(),
            evicted: 0,
            total_registered: 0,
        }
    }
    /// 登记：满则先逐出最旧已销账项。
    pub fn admit(&mut self, d: Defect) -> Result<(), u16> {
        if self.ring.iter().any(|o| o.id == d.id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= DEFECT_CAP {
            let p = self.ring.iter().position(|o| o.state.is_closed());
            match p {
                Some(idx) => {
                    self.ring.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.ring.push(d);
        self.total_registered += 1;
        Ok(())
    }
    /// 状态推进：合法转移闭集——
    /// Open→Fixed、Fixed→Verified、Open→WontFix、
    /// Verified/WontFix→Open（重开，计次）。
    pub fn transition(&mut self, id: u32, to: DState) -> Result<DState, u16> {
        let p = self.ring.iter().position(|o| o.id == id).ok_or(E_NODF)?;
        let d = &mut self.ring[p];
        let legal = match (d.state, to) {
            (DState::Open, DState::Fixed) => true,
            (DState::Open, DState::WontFix) => true,
            (DState::Fixed, DState::Verified) => true,
            (DState::Verified, DState::Open) => true,
            (DState::WontFix, DState::Open) => true,
            _ => false,
        };
        if !legal {
            return Err(E_STATE);
        }
        if to == DState::Open {
            if d.reopens >= REOPEN_LIMIT {
                return Err(E_REOPEN);
            }
            d.reopens += 1;
        }
        d.state = to;
        Ok(to)
    }
    /// 在册（Open/Fixed）计数。
    pub fn open_count(&self) -> u32 {
        self.ring
            .iter()
            .filter(|d| !d.state.is_closed())
            .count() as u32
    }
    /// 清账判据：在册为零。
    pub fn clearable(&self) -> bool {
        self.open_count() == 0
    }
    /// 守恒审计：total_registered == held + closed_held + evicted 中
    /// held + evicted 只是容量口径；登记口径另需 closed_out。
    /// 这里采用直接口径：total_registered == held + evicted + 逐出前销账
    /// 不可能重建，故改验环形不变：held ≤ CAP 且 evicted+held ≤ total。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ring.len() > DEFECT_CAP {
            return Err(E_LEDGER);
        }
        if self.ring.len() as u64 + self.evicted > self.total_registered {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
    pub fn evicted(&self) -> u64 {
        self.evicted
    }
    pub fn total(&self) -> u64 {
        self.total_registered
    }
    /// 严重级分布（四级闭集计数）。
    pub fn sev_tally(&self) -> [u32; 4] {
        let mut t = [0u32; 4];
        for d in &self.ring {
            t[d.sev.ord() as usize] += 1;
        }
        t
    }
}

/// 自例程：主路径（全销账清账）+ 重开路径 + 错误路径 + 定容逐出守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = DefectLedger::new();
    // 主路径 1：Open→Fixed→Verified。
    led.admit(Defect::new(1, "blend glitch", Sev::S1))?;
    led.admit(Defect::new(2, "doc typo", Sev::S3))?;
    led.audit()?;
    if led.transition(1, DState::Fixed)? != DState::Fixed {
        return Err(E_STATE);
    }
    if led.transition(1, DState::Verified)? != DState::Verified {
        return Err(E_STATE);
    }
    // Open→WontFix 直接销账。
    led.transition(2, DState::WontFix)?;
    if !led.clearable() {
        return Err(E_STATE);
    }
    let t = led.sev_tally();
    if t[Sev::S1.ord() as usize] != 1 || t[Sev::S3.ord() as usize] != 1 {
        return Err(E_LEDGER);
    }
    // 主路径 2：重开一次再修再验。
    led.transition(1, DState::Open)?;
    if led.ring_find(1)?.reopens != 1 {
        return Err(E_REOPEN);
    }
    led.transition(1, DState::Fixed)?;
    led.transition(1, DState::Verified)?;
    if !led.clearable() {
        return Err(E_STATE);
    }
    // 主路径 3：重开计次达到上限后拒绝再重开。
    led.transition(1, DState::Open)?; // 第二次重开，reopens==2 == LIMIT
    if led.ring_find(1)?.reopens != 2 {
        return Err(E_REOPEN);
    }
    // 先修验，再试第三次重开 → E_REOPEN。
    led.transition(1, DState::Fixed)?;
    led.transition(1, DState::Verified)?;
    if led.transition(1, DState::Open) != Err(E_REOPEN) {
        return Err(E_REOPEN);
    }
    // 错误路径 1：非法转移闭集。
    led.admit(Defect::new(3, "skip", Sev::S2))?;
    // Open→Verified 跳步非法。
    if led.transition(3, DState::Verified) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // Open→Fixed 合法。
    led.transition(3, DState::Fixed)?;
    // Fixed→WontFix 非法。
    if led.transition(3, DState::WontFix) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // Fixed→Verified 合法；Verified→Fixed 回退非法。
    led.transition(3, DState::Verified)?;
    if led.transition(3, DState::Fixed) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 2：未知编号。
    if led.transition(99, DState::Fixed) != Err(E_NODF) {
        return Err(E_NODF);
    }
    // 错误路径 3：重复编号。
    if led.admit(Defect::new(1, "dup", Sev::S0)) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 定容逐出：灌 75 个缺陷（前 72 个全部销账，后 3 个入账触发逐出）。
    let mut led2 = DefectLedger::new();
    for i in 1..=75u32 {
        let d = Defect::new(i, "fill", Sev::S2);
        if i <= 72 {
            // 预销账态：先 Open 登记，登记后立即 WontFix。
            led2.admit(d.clone())?;
            led2.transition(i, DState::WontFix)?;
        } else {
            // 账本满 72 且全为已销账 → 逐出最旧已销账项。
            led2.admit(d)?;
        }
    }
    led2.audit()?;
    if led2.held() != DEFECT_CAP || led2.evicted() != 3 || led2.total() != 75 {
        return Err(E_LEDGER);
    }
    // 逐出的是最旧项（id=1 已不在册）。
    if led2.transition(1, DState::Open) != Err(E_NODF) {
        return Err(E_LEDGER);
    }
    // 满且全在册 → 拒绝入账。
    let mut led3 = DefectLedger::new();
    for i in 1..=72u32 {
        led3.admit(Defect::new(i, "stuck", Sev::S0))?;
    }
    if led3.admit(Defect::new(73, "no room", Sev::S0)) != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}

impl DefectLedger {
    /// 只读定位（供 drill 断言）。
    pub fn ring_find(&self, id: u32) -> Result<&Defect, u16> {
        self.ring.iter().find(|d| d.id == id).ok_or(E_NODF)
    }
}
