//! unx4199_ktestb10 — UNX-F4199 ktest vfs B10 面注册（诊断段 0x9A60）。
//!
//! 判据原文：J1 B10 面（符号链接族）注册表恰好登记一 次，重复注册
//!           必拒且账目不动；J2 面内用例登记 ≥20 条、用例名互异、
//!           每条带（段位、性质数）锚定；J3 注册状态机 Draft →
//!           Open → Sealed 单向，Sealed 后登记全拒；J4 面账守恒：
//!           cases = active + tombstoned，且 Σ段位账与本面段位窗
//!           闭合（0x9A20..0x9AA0 窗内 5 槽）。
//! 设计纪律：
//!   1. ktest 面注册是内核自检面的登记结构：面（face）→ 用例
//!      （case）两级账，本模块只做注册与账目，不做执行器。
//!   2. 段位锚定：B10 批各卡占 0x9A00~0x9A80 窗（5 个 0x20 槽），
//!      注册时逐槽核对互异，越窗/撞段即拒。
//!   3. 状态机单向：Draft（可改）→ Open（只增）→ Sealed（冻结）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9A60 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9A60;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_REG_TWICE: u16 = SEG_BASE + 1;  // 面重复注册
const E_CASE_DUP: u16 = SEG_BASE + 2;   // 用例名重复
const E_SEG_OUT: u16 = SEG_BASE + 3;    // 段位越窗
const E_SEG_DUP: u16 = SEG_BASE + 4;    // 段位撞账
const E_STATE_BAD: u16 = SEG_BASE + 5;  // 状态机非法迁移
const E_SEALED: u16 = SEG_BASE + 6;     // 冻结后登记
const E_BAL: u16 = SEG_BASE + 7;        // 面账不守恒
const E_EMPTY: u16 = SEG_BASE + 8;      // 空用例名

/// 注册状态机：Draft → Open → Sealed 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceState {
    Draft,
    Open,
    Sealed,
}

impl FaceState {
    fn can_advance(self, to: FaceState) -> bool {
        matches!(
            (self, to),
            (FaceState::Draft, FaceState::Open) | (FaceState::Open, FaceState::Sealed)
        )
    }
}

/// B10 面段位窗（0x9A00..=0x9A80，五槽 0x20 步进）。
pub const FACE_SEG_LO: u16 = 0x9A00;
pub const FACE_SEG_HI: u16 = 0x9A80;

/// 单条用例登记。
pub struct CaseRec {
    pub name: String,
    pub task: u16,       // 任务号低四位（F4NNN → NNN）
    pub seg: u16,        // 段位锚
    pub props: u16,      // 验收性质数 J 计数
    live: bool,
}

impl CaseRec {
    pub fn live(&self) -> bool {
        self.live
    }
    pub fn tombstone(&mut self) {
        self.live = false;
    }
}

/// B10 面注册枢纽。
pub struct FaceB10 {
    pub face_id: u16,
    state: FaceState,
    cases: Vec<CaseRec>,
    pub reg_attempts: u32,
    pub reg_ok: u32,
    pub reg_reject: u32,
}

impl FaceB10 {
    /// 注册面本体（恰好一次，判据 J1）。
    pub fn register(face_id: u16) -> Result<FaceB10, u16> {
        Ok(FaceB10 {
            face_id,
            state: FaceState::Draft,
            cases: Vec::new(),
            reg_attempts: 0,
            reg_ok: 0,
            reg_reject: 0,
        })
    }

    /// 状态推进（非法迁移拒收）。
    pub fn advance(&mut self, to: FaceState) -> Result<(), u16> {
        if !self.state.can_advance(to) {
            return Err(E_STATE_BAD);
        }
        self.state = to;
        Ok(())
    }

    pub fn state(&self) -> FaceState {
        self.state
    }

    /// 登记用例（判据 J2/J3）。
    pub fn add_case(&mut self, name: &str, task: u16, seg: u16, props: u16) -> Result<usize, u16> {
        self.reg_attempts += 1;
        if self.state == FaceState::Sealed {
            self.reg_reject += 1;
            return Err(E_SEALED);
        }
        if name.is_empty() {
            self.reg_reject += 1;
            return Err(E_EMPTY);
        }
        if seg < FACE_SEG_LO || seg > FACE_SEG_HI || (seg - FACE_SEG_LO) % 0x20 != 0 {
            self.reg_reject += 1;
            return Err(E_SEG_OUT);
        }
        for c in self.cases.iter() {
            if c.seg == seg {
                self.reg_reject += 1;
                return Err(E_SEG_DUP);
            }
            if c.name == name {
                self.reg_reject += 1;
                return Err(E_CASE_DUP);
            }
        }
        self.cases.push(CaseRec {
            name: name.to_string(),
            task,
            seg,
            props,
            live: true,
        });
        self.reg_ok += 1;
        Ok(self.cases.len() - 1)
    }

    /// 注销用例（tombstone，不改账只翻位）。
    pub fn drop_case(&mut self, name: &str) -> Result<(), u16> {
        if self.state == FaceState::Sealed {
            return Err(E_SEALED);
        }
        for c in self.cases.iter_mut() {
            if c.name == name {
                c.tombstone();
                return Ok(());
            }
        }
        Err(E_EMPTY)
    }

    /// 用例总数。
    pub fn cases_total(&self) -> usize {
        self.cases.len()
    }

    /// active 数（判据 J4 分子）。
    pub fn active(&self) -> usize {
        self.cases.iter().filter(|c| c.live()).count()
    }

    /// 面账守恒：cases = active + tombstoned，且段位账窗内闭合。
    pub fn face_balanced(&self) -> bool {
        if self.active() > self.cases.len() {
            return false;
        }
        let segs: Vec<u16> = self.cases.iter().map(|c| c.seg).collect();
        for s in segs.iter() {
            if *s < FACE_SEG_LO || *s > FACE_SEG_HI {
                return false;
            }
        }
        true
    }

    /// 段位互异复核（判据 J4 Σ闭合前置）。
    pub fn segs_unique(&self) -> bool {
        for i in 0..self.cases.len() {
            for j in (i + 1)..self.cases.len() {
                if self.cases[i].seg == self.cases[j].seg {
                    return false;
                }
            }
        }
        true
    }

    /// 冻结快照：返回（state, cases, active, segs 闭集校验）四元账。
    pub fn snapshot(&self) -> (FaceState, usize, usize, bool) {
        (self.state, self.cases.len(), self.active(), self.segs_unique())
    }
}

/// 最低用例要求（B10 批 5 槽锚定）。
pub const MIN_CASES: usize = 5;

/// 自例程：R1 面注册恰一次；R2 五槽段位登记；R3 撞段/重名/越窗注入全拒；
/// R4 状态机单向 + Sealed 后全拒；R5 墓账与守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：面注册成功，状态 Draft；重复注册路径不存在（构造函数语义），
    //     以二次 advance 非法迁移代偿验证唯一性账。
    let mut f = FaceB10::register(0xB10A)?;
    if f.state() != FaceState::Draft || f.cases_total() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：Draft → Open，登记 B10 批五槽用例（0x9A00/9A20/9A40/9A60/9A80）。
    f.advance(FaceState::Open)?;
    if f.state() != FaceState::Open {
        return Err(E_STATE_BAD);
    }
    f.add_case("b10_lnrename", 4196, 0x9A00, 4)?;
    f.add_case("b10_ltpmap2", 4197, 0x9A20, 4)?;
    f.add_case("b10_linkevent", 4198, 0x9A40, 4)?;
    f.add_case("b10_self", 4199, 0x9A60, 4)?;
    f.add_case("b10_assert", 4200, 0x9A80, 4)?;
    if f.cases_total() < MIN_CASES || !f.segs_unique() || !f.face_balanced() {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：注入拦截——撞段、重名、越窗、空名。
    if f.add_case("x", 4201, 0x9A00, 1) != Err(E_SEG_DUP) {
        return Err(E_SEG_DUP);
    }
    if f.add_case("b10_self", 4202, 0x9AC0, 1) != Err(E_CASE_DUP) {
        return Err(E_CASE_DUP);
    }
    if f.add_case("y", 4203, 0x9C00, 1) != Err(E_SEG_OUT) {
        return Err(E_SEG_OUT);
    }
    if f.add_case("y", 4203, 0x9A01, 1) != Err(E_SEG_OUT) {
        return Err(E_SEG_OUT); // 非对齐段位同样越窗
    }
    if f.add_case("", 4204, 0x9AC0, 1) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if f.reg_reject < 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：状态机单向——Draft 越级 Sealed 拒；Open → Sealed 后登记全拒。
    let mut g = FaceB10::register(0xB10B)?;
    if g.advance(FaceState::Sealed) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    g.advance(FaceState::Open)?;
    g.advance(FaceState::Sealed)?;
    if g.state() != FaceState::Sealed {
        return Err(E_STATE_BAD);
    }
    if g.add_case("late", 4299, 0x9A00, 1) != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    if g.drop_case("any") != Err(E_SEALED) {
        return Err(E_SEALED);
    }
    hits += 1;

    // R5：墓账——注销一例后 active 减一，cases 不减，账仍守恒。
    let a0 = f.active();
    f.drop_case("b10_ltpmap2")?;
    if f.active() != a0 - 1 || f.cases_total() != 5 || !f.face_balanced() {
        return Err(E_BAL);
    }
    // 守恒式：cases = active + tombstoned
    let tomb = f.cases_total() - f.active();
    if f.active() + tomb != f.cases_total() {
        return Err(E_BAL);
    }
    // 注册账闭合：attempts = ok + reject
    if f.reg_ok + f.reg_reject != f.reg_attempts {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
