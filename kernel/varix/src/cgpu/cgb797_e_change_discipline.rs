//! CGPU-F0797 · E 域变更纪律（目标 300 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0797，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：E 域变更纪律——变更请求（ChangeRequest）全流程管控：
//!   提案→评审→批准/驳回→落地，未批准不得落地，冻结窗内一律驳回，
//!   变更账本定容守恒；冻结窗判定用整数刻度。
//!
//! 设计纪律：
//! - 变更类别闭集三类：API（接口变更，需双批）/ Perf（性能取舍，
//!   单批）/ Doc（文档更正，单批）；API 类必须两个批准者。
//! - 状态机闭集：Proposed→Reviewed→Approved→Landed；
//!   Proposed→Reviewed→Rejected；Approved 24 刻度内未落地自动过期
//!   （Expire 口径：approve_tick + TTL ≤ now_tick）。
//! - 冻结窗：FREEZE_WINDOW = [950, 1000]（刻度闭区间），
//!   窗内新提案直接驳回（Discipline 判定）。
//! - 变更账本定容：CR_CAP=56，超容逐出最旧终态项（无则拒绝），
//!   守恒恒等式：total_crs == held + evicted。
//! - 纯功能线：no_std + alloc，整数算术，确定性；
//!   诊断段 0x6EE0 本单独占，错误码 SEG_BASE+1..+N 闭集。

#![allow(dead_code)]
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 诊断段基址（0x6EE0，批内 CGPU-F0797 独占）。
pub const SEG_BASE: u16 = 0x6EE0;

/// 变更编号重复。
pub const E_DUP: u16 = SEG_BASE + 1;
/// 变更账本已满且无可逐出终态项。
pub const E_CAP: u16 = SEG_BASE + 2;
/// 状态机非法转移。
pub const E_STATE: u16 = SEG_BASE + 3;
/// 未知变更编号。
pub const E_NOCR: u16 = SEG_BASE + 4;
/// 签批数不足（API 类需双批）。
pub const E_SIGN: u16 = SEG_BASE + 5;
/// 冻结窗内禁止新提案。
pub const E_FREEZE: u16 = SEG_BASE + 6;
/// 批准已过期，不得落地。
pub const E_EXPIRED: u16 = SEG_BASE + 7;
/// 守恒破坏。
pub const E_LEDGER: u16 = SEG_BASE + 8;

/// 变更账本容量。
pub const CR_CAP: usize = 56;
/// 批准有效期（刻度）。
pub const APPROVE_TTL: u64 = 24;
/// 冻结窗（闭区间）。
pub const FREEZE_LO: u64 = 950;
pub const FREEZE_HI: u64 = 1000;

/// 变更类别（闭集三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrKind {
    Api,
    Perf,
    Doc,
}

impl CrKind {
    /// 是否需要双批。
    pub fn dual_sign(self) -> bool {
        self == CrKind::Api
    }
    pub fn ord(self) -> u8 {
        match self {
            CrKind::Api => 0,
            CrKind::Perf => 1,
            CrKind::Doc => 2,
        }
    }
}

/// 变更状态（闭集六态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrState {
    Proposed,
    Reviewed,
    Approved,
    Landed,
    Rejected,
    Expired,
}

impl CrState {
    pub fn is_final(self) -> bool {
        matches!(
            self,
            CrState::Landed | CrState::Rejected | CrState::Expired
        )
    }
    pub fn ord(self) -> u8 {
        match self {
            CrState::Proposed => 0,
            CrState::Reviewed => 1,
            CrState::Approved => 2,
            CrState::Landed => 3,
            CrState::Rejected => 4,
            CrState::Expired => 5,
        }
    }
}

/// 变更请求。
#[derive(Clone, Debug)]
pub struct ChangeRequest {
    /// 编号。
    pub id: u32,
    /// 标题。
    pub title: String,
    /// 类别。
    pub kind: CrKind,
    /// 状态。
    pub state: CrState,
    /// 签批数。
    pub signs: u32,
    /// 批准时刻。
    pub approve_tick: u64,
}

/// 变更账本。
#[derive(Clone, Debug)]
pub struct CrLedger {
    ring: Vec<ChangeRequest>,
    evicted: u64,
    total_crs: u64,
    frozen: bool,
}

impl CrLedger {
    pub fn new() -> Self {
        CrLedger {
            ring: Vec::new(),
            evicted: 0,
            total_crs: 0,
            frozen: false,
        }
    }
    /// 冻结窗开关（刻度落入 [FREEZE_LO, FREEZE_HI] 即开）。
    pub fn set_freeze(&mut self, tick: u64) {
        self.frozen = tick >= FREEZE_LO && tick <= FREEZE_HI;
    }
    /// 新提案：冻结窗内拒绝。
    pub fn propose(&mut self, id: u32, title: &str, kind: CrKind) -> Result<(), u16> {
        if title.is_empty() {
            return Err(E_STATE);
        }
        if self.frozen {
            return Err(E_FREEZE);
        }
        if self.ring.iter().any(|c| c.id == id) {
            return Err(E_DUP);
        }
        if self.ring.len() >= CR_CAP {
            let p = self.ring.iter().position(|c| c.state.is_final());
            match p {
                Some(idx) => {
                    self.ring.remove(idx);
                    self.evicted += 1;
                }
                None => return Err(E_CAP),
            }
        }
        self.ring.push(ChangeRequest {
            id,
            title: title.to_string(),
            kind,
            state: CrState::Proposed,
            signs: 0,
            approve_tick: 0,
        });
        self.total_crs += 1;
        Ok(())
    }
    fn pos(&self, id: u32) -> Result<usize, u16> {
        self.ring.iter().position(|c| c.id == id).ok_or(E_NOCR)
    }
    /// 送审：Proposed→Reviewed。
    pub fn review(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != CrState::Proposed {
            return Err(E_STATE);
        }
        self.ring[p].state = CrState::Reviewed;
        Ok(())
    }
    /// 签批：Reviewed 态累计；API 类需 2 票，其余 1 票。
    pub fn sign(&mut self, id: u32, tick: u64) -> Result<CrState, u16> {
        let p = self.pos(id)?;
        let c = &mut self.ring[p];
        if c.state != CrState::Reviewed {
            return Err(E_STATE);
        }
        c.signs += 1;
        let need = if c.kind.dual_sign() { 2 } else { 1 };
        if c.signs >= need {
            c.state = CrState::Approved;
            c.approve_tick = tick;
        }
        Ok(c.state)
    }
    /// 落地：Approved 且未过期（tick ≤ approve_tick + TTL）。
    pub fn land(&mut self, id: u32, now_tick: u64) -> Result<(), u16> {
        let p = self.pos(id)?;
        let c = &mut self.ring[p];
        if c.state != CrState::Approved {
            return Err(E_STATE);
        }
        if now_tick > c.approve_tick.saturating_add(APPROVE_TTL) {
            c.state = CrState::Expired;
            return Err(E_EXPIRED);
        }
        c.state = CrState::Landed;
        Ok(())
    }
    /// 驳回：Reviewed → Rejected。
    pub fn reject(&mut self, id: u32) -> Result<(), u16> {
        let p = self.pos(id)?;
        if self.ring[p].state != CrState::Reviewed {
            return Err(E_STATE);
        }
        self.ring[p].state = CrState::Rejected;
        Ok(())
    }
    /// 定期清扫：过期已批准项（tick > approve+TTL）置 Expired。
    pub fn sweep(&mut self, now_tick: u64) -> u32 {
        let mut n = 0u32;
        for c in self.ring.iter_mut() {
            if c.state == CrState::Approved
                && now_tick > c.approve_tick.saturating_add(APPROVE_TTL)
            {
                c.state = CrState::Expired;
                n += 1;
            }
        }
        n
    }
    /// 终态计数。
    pub fn final_count(&self) -> u32 {
        self.ring.iter().filter(|c| c.state.is_final()).count() as u32
    }
    /// 守恒审计：total_crs == held + evicted。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total_crs != self.ring.len() as u64 + self.evicted {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    pub fn held(&self) -> usize {
        self.ring.len()
    }
}

/// 自例程：主路径（Doc/Perf 单批落地、Api 双批落地）+ 冻结/过期/错误路径 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut led = CrLedger::new();
    // 主路径 1：Doc 单批落地。
    led.propose(1, "fix typo", CrKind::Doc)?;
    led.review(1)?;
    if led.sign(1, 10)? != CrState::Approved {
        return Err(E_SIGN);
    }
    led.land(1, 20)?;
    // 主路径 2：Perf 单批落地。
    led.propose(2, "blend fastpath", CrKind::Perf)?;
    led.review(2)?;
    led.sign(2, 10)?;
    led.land(2, 34)?; // 恰好 TTL 边界内
    // 主路径 3：Api 双批——一票不成，两票成。
    led.propose(3, "api widen", CrKind::Api)?;
    led.review(3)?;
    if led.sign(3, 10)? != CrState::Reviewed {
        return Err(E_SIGN);
    }
    if led.land(3, 11) != Err(E_STATE) {
        return Err(E_STATE);
    }
    if led.sign(3, 10)? != CrState::Approved {
        return Err(E_SIGN);
    }
    led.land(3, 12)?;
    led.audit()?;
    if led.final_count() != 3 {
        return Err(E_LEDGER);
    }
    // 冻结窗：刻度 950..1000 内提案拒绝。
    led.set_freeze(975);
    if led.propose(9, "in freeze", CrKind::Doc) != Err(E_FREEZE) {
        return Err(E_FREEZE);
    }
    led.set_freeze(949);
    led.propose(9, "before freeze", CrKind::Doc)?;
    led.set_freeze(1000);
    if led.propose(10, "edge freeze", CrKind::Doc) != Err(E_FREEZE) {
        return Err(E_FREEZE);
    }
    led.set_freeze(1001);
    led.propose(10, "after freeze", CrKind::Doc)?;
    // 过期：批准后 25 刻度落地 → 拒绝并置 Expired。
    led.review(9)?;
    led.sign(9, 100)?;
    if led.land(9, 100 + APPROVE_TTL + 1) != Err(E_EXPIRED) {
        return Err(E_EXPIRED);
    }
    // sweep 清扫：再批一件不放，sweep 后 Expired。
    led.review(10)?;
    led.sign(10, 100)?;
    let n = led.sweep(200);
    if n != 1 || led.final_count() != 5 {
        return Err(E_LEDGER);
    }
    // 错误路径 1：重复编号。
    if led.propose(1, "dup", CrKind::Doc) != Err(E_DUP) {
        return Err(E_DUP);
    }
    // 错误路径 2：状态跳步（Proposed 直接签批/落地）。
    led.propose(11, "fresh", CrKind::Perf)?;
    if led.sign(11, 1) != Err(E_STATE) || led.land(11, 1) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 错误路径 3：Rejected 后再签批。
    led.review(11)?;
    led.reject(11)?;
    if led.sign(11, 2) != Err(E_STATE) || !led.ring_state(11)?.is_final() {
        return Err(E_STATE);
    }
    // 错误路径 4：未知编号。
    if led.review(99) != Err(E_NOCR) {
        return Err(E_NOCR);
    }
    // 错误路径 5：空标题。
    if led.propose(12, "", CrKind::Doc) != Err(E_STATE) {
        return Err(E_STATE);
    }
    // 定容逐出：先灌 56 个全部 Landed 的变更，再入 2 个触发逐出。
    let mut led2 = CrLedger::new();
    for i in 1..=56u32 {
        led2.propose(i, "fill", CrKind::Doc)?;
        led2.review(i)?;
        led2.sign(i, 1)?;
        led2.land(i, 2)?;
    }
    led2.propose(57, "extra1", CrKind::Doc)?;
    led2.propose(58, "extra2", CrKind::Doc)?;
    led2.audit()?;
    if led2.held() != CR_CAP || led2.evicted != 2 || led2.final_count() != 54 {
        return Err(E_LEDGER);
    }
    // 满且全在册（非终态）→ 拒绝。
    let mut led3 = CrLedger::new();
    for i in 1..=56u32 {
        led3.propose(i, "stuck", CrKind::Doc)?;
    }
    if led3.propose(57, "no room", CrKind::Doc) != Err(E_CAP) {
        return Err(E_CAP);
    }
    Ok(0)
}

impl CrLedger {
    /// 只读状态查询。
    pub fn ring_state(&self, id: u32) -> Result<CrState, u16> {
        self.ring
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.state)
            .ok_or(E_NOCR)
    }
}
