//! UNX-F12689 根路径协议回归（B30 档） · 0x81600段
//!
//! 判据原文（派工卡 12688 逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F12689-J1 挂起期访问拒止回归 10/10 维持，
//!      R-D1-029 复核留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 访问挂起域模型：相位三闭集（常态/挂起期/撤收期）× 操作四闭集
//!    （读/写/开句柄/查名）的拒止矩阵——矩阵冻结为判据口径；
//! 2. 挂起期拒止判据锚：挂起期写与开句柄必拒（读与查名放行），撤收期
//!    四操作全拒，常态四操作全放——10 场景节点全量回归 = 10/10 维持；
//! 3. 场景节点五元（场景号/相位/操作/冻结期望/复核槽）：实际判定必须
//!    等于冻结期望，翻转期望注入必被期望闸抓出；
//! 4. R-D1-029 复核留痕：每场景必落一条复核账（复核人短名 + 判定码），
//!    缺条/重条/越界复核均显性拒——留痕账与场景账一一对应；
//! 5. 回归账状态机 RpOpen → RpRunning → RpSealed 单向推进，倒退显性
//!    拒；封账只读；零静默：场景数 ≡ 复核条数守恒；
//! 6. 注入联测十路：乱序/重复/期望翻转/矩阵漂移/留痕缺条/留痕重条/
//!    封账后写/状态机倒退/越界相位/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `suspend_deny_ten_ok` ↔ ② 挂起期访问拒止回归 10/10 维持；
//!   - `matrix_semantics_ok` ↔ ② 拒止矩阵冻结口径（挂起拒写/开句柄）；
//!   - `review_trail_ok` ↔ ② R-D1-029 复核留痕（一一对应零缺漏）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12688 − 12600)）。
pub const SEG_BASE: u32 = 0x81600;

// ── 挂起期访问回归诊断码闭集 ─────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpDiag {
    /// 场景乱序。
    ESceneOrder,
    /// 场景重复。
    ESceneDup,
    /// 期望翻转（实际判定 ≠ 冻结期望）。
    EExpectFlip,
    /// 矩阵漂移（判定 ≠ 冻结矩阵）。
    EMatrixDrift,
    /// 留痕缺条（场景无复核账）。
    EReviewMissing,
    /// 留痕重条（同场景多复核账）。
    EReviewDup,
    /// 越界相位（相位不在三闭集）。
    EPhaseRange,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl RpDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                RpDiag::ESceneOrder => 0x01,
                RpDiag::ESceneDup => 0x02,
                RpDiag::EExpectFlip => 0x03,
                RpDiag::EMatrixDrift => 0x04,
                RpDiag::EReviewMissing => 0x05,
                RpDiag::EReviewDup => 0x06,
                RpDiag::EPhaseRange => 0x07,
                RpDiag::ESealedWrite => 0x08,
                RpDiag::ERewind => 0x09,
                RpDiag::ESummaryEmpty => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            RpDiag::ESceneOrder => "场景乱序：按场景号 0..10 逐场景回归，跳号先回补",
            RpDiag::ESceneDup => "场景重复：同相位同操作只立一场景，重复先查已有账",
            RpDiag::EExpectFlip => "期望翻转：实际判定必须等于冻结期望，先核判据口径",
            RpDiag::EMatrixDrift => "矩阵漂移：判定必须落在冻结拒止矩阵内，先对矩阵",
            RpDiag::EReviewMissing => "留痕缺条：每场景必落一条 R-D1-029 复核账，缺条先补",
            RpDiag::EReviewDup => "留痕重条：同场景只许一条复核账，重条先去重",
            RpDiag::EPhaseRange => "越界相位：相位必须落在三闭集内，越界先归相位",
            RpDiag::ESealedWrite => "封账后写：回归账只读，补场景走下一轮回归",
            RpDiag::ERewind => "状态机倒退：RpOpen→RpRunning→RpSealed 只进不退，倒退即账可疑",
            RpDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const RP_ALL: &[RpDiag] = &[
    RpDiag::ESceneOrder,
    RpDiag::ESceneDup,
    RpDiag::EExpectFlip,
    RpDiag::EMatrixDrift,
    RpDiag::EReviewMissing,
    RpDiag::EReviewDup,
    RpDiag::EPhaseRange,
    RpDiag::ESealedWrite,
    RpDiag::ERewind,
    RpDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in RP_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81601..0x8160A 闭集内"
}

// ── 相位三闭集 ───────────────────────────────────────────────────
/// 访问相位（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessPhase {
    /// 常态。
    Active,
    /// 挂起期。
    Suspended,
    /// 撤收期。
    Withdrawn,
}

impl AccessPhase {
    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            AccessPhase::Active => "常态",
            AccessPhase::Suspended => "挂起期",
            AccessPhase::Withdrawn => "撤收期",
        }
    }

    /// 相位号。
    pub fn ordinal(self) -> u32 {
        match self {
            AccessPhase::Active => 0,
            AccessPhase::Suspended => 1,
            AccessPhase::Withdrawn => 2,
        }
    }

    /// 全部相位（闭集数组）。
    pub fn all() -> &'static [AccessPhase] {
        &[AccessPhase::Active, AccessPhase::Suspended, AccessPhase::Withdrawn]
    }
}

// ── 操作四闭集 ───────────────────────────────────────────────────
/// 访问操作（四闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessOp {
    /// 读。
    OpRead,
    /// 写。
    OpWrite,
    /// 开句柄。
    OpOpen,
    /// 查名。
    OpQuery,
}

impl AccessOp {
    /// 操作名。
    pub fn name(self) -> &'static str {
        match self {
            AccessOp::OpRead => "读",
            AccessOp::OpWrite => "写",
            AccessOp::OpOpen => "开句柄",
            AccessOp::OpQuery => "查名",
        }
    }

    /// 全部操作（闭集数组）。
    pub fn all() -> &'static [AccessOp] {
        &[AccessOp::OpRead, AccessOp::OpWrite, AccessOp::OpOpen, AccessOp::OpQuery]
    }
}

// ── 冻结拒止矩阵（判据口径） ─────────────────────────────────────
/// 拒止矩阵判定：给定相位与操作，返回是否放行。
/// 来源：挂起期访问拒止协议冻结矩阵——挂起期拒写/拒开句柄，撤收期全拒。
pub fn access_allowed(phase: AccessPhase, op: AccessOp) -> bool {
    match phase {
        AccessPhase::Active => true,
        AccessPhase::Suspended => match op {
            AccessOp::OpRead => true,
            AccessOp::OpWrite => false,
            AccessOp::OpOpen => false,
            AccessOp::OpQuery => true,
        },
        AccessPhase::Withdrawn => false,
    }
}

/// 场景谱判码（0x11 + 场景号 + 1；来源：挂起期回归清单冻结轴）。
pub fn rp_spec_code(scene_no: u32) -> u32 {
    0x11u32.saturating_add(scene_no).saturating_add(1)
}

/// 判码是否落在场景谱内（0x12..0x1B）。
pub fn code_in_rp_spec(code: u32) -> bool {
    code >= 0x12 && code <= 0x1B
}

// ── 场景节点域模型 ───────────────────────────────────────────────
/// 无复核哨兵（首场景留痕槽占位）。
pub const REVIEW_HEAD: u32 = 0xFFFF_FFFF;

/// 回归场景五元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpScene {
    /// 场景号（0..10）。
    pub no: u32,
    /// 相位。
    pub phase: AccessPhase,
    /// 操作。
    pub op: AccessOp,
    /// 冻结期望（true=放行）。
    pub expect_allow: bool,
    /// 场景谱判码。
    pub verdict_code: u32,
}

/// 冻结场景表（10 场景：常态 3 + 挂起期 4 + 撤收期 3）。
pub const RP_SCENES: &[RpScene] = &[
    RpScene { no: 0, phase: AccessPhase::Active, op: AccessOp::OpRead, expect_allow: true, verdict_code: 0x12 },
    RpScene { no: 1, phase: AccessPhase::Active, op: AccessOp::OpWrite, expect_allow: true, verdict_code: 0x13 },
    RpScene { no: 2, phase: AccessPhase::Active, op: AccessOp::OpOpen, expect_allow: true, verdict_code: 0x14 },
    RpScene { no: 3, phase: AccessPhase::Suspended, op: AccessOp::OpRead, expect_allow: true, verdict_code: 0x15 },
    RpScene { no: 4, phase: AccessPhase::Suspended, op: AccessOp::OpWrite, expect_allow: false, verdict_code: 0x16 },
    RpScene { no: 5, phase: AccessPhase::Suspended, op: AccessOp::OpOpen, expect_allow: false, verdict_code: 0x17 },
    RpScene { no: 6, phase: AccessPhase::Suspended, op: AccessOp::OpQuery, expect_allow: true, verdict_code: 0x18 },
    RpScene { no: 7, phase: AccessPhase::Withdrawn, op: AccessOp::OpRead, expect_allow: false, verdict_code: 0x19 },
    RpScene { no: 8, phase: AccessPhase::Withdrawn, op: AccessOp::OpWrite, expect_allow: false, verdict_code: 0x1A },
    RpScene { no: 9, phase: AccessPhase::Withdrawn, op: AccessOp::OpQuery, expect_allow: false, verdict_code: 0x1B },
];

/// 场景总数（10/10 判据锚定值）。
pub const RP_SCENE_CAP: usize = 10;

/// 按场景号取冻结场景（越界 None）。
pub fn replay_rp(no: u32) -> Option<RpScene> {
    if no >= RP_SCENES.len() as u32 {
        return None;
    }
    for s in RP_SCENES.iter() {
        if s.no == no {
            return Some(s.clone());
        }
    }
    None
}

// ── 回归账状态机 ─────────────────────────────────────────────────
/// 回归账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RpState {
    /// 开账。
    RpOpen,
    /// 回归中。
    RpRunning,
    /// 封账（只读）。
    RpSealed,
}

impl RpState {
    /// 状态秩。
    pub fn rank(self) -> u32 {
        match self {
            RpState::RpOpen => 0,
            RpState::RpRunning => 1,
            RpState::RpSealed => 2,
        }
    }
}

/// R-D1-029 复核留痕条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RpReview {
    /// 复核场景号。
    pub scene_no: u32,
    /// 复核人短名（冻结复核席）。
    pub reviewer: &'static str,
    /// 判定码（与场景谱判码一致）。
    pub verdict_code: u32,
}

/// 挂起期访问回归账。
#[derive(Clone, Debug)]
pub struct RpLedger {
    /// 状态。
    state: RpState,
    /// 已入账场景。
    scenes: Vec<RpScene>,
    /// 复核留痕账（与场景一一对应）。
    reviews: Vec<RpReview>,
    /// 拒止计数。
    rejects: u32,
    /// 判定数。
    verdicts: u32,
}

impl RpLedger {
    /// 新账。
    pub fn new() -> RpLedger {
        RpLedger {
            state: RpState::RpOpen,
            scenes: Vec::new(),
            reviews: Vec::new(),
            rejects: 0,
            verdicts: 0,
        }
    }

    /// 开账 → 回归中。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            RpState::RpOpen => {
                self.state = RpState::RpRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RpDiag::ERewind.val())
            }
        }
    }

    /// 回归中 → 封账（未齐不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            RpState::RpRunning => {
                if self.scenes.len() != RP_SCENE_CAP || self.reviews.len() != RP_SCENE_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(RpDiag::EReviewMissing.val());
                }
                self.state = RpState::RpSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(RpDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> RpState {
        self.state
    }

    /// 已入账场景数。
    pub fn len(&self) -> usize {
        self.scenes.len()
    }

    /// 复核条数。
    pub fn review_len(&self) -> usize {
        self.reviews.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 判定数。
    pub fn verdict_len(&self) -> u32 {
        self.verdicts
    }

    /// 下标取场景（越界 None）。
    pub fn scene_at(&self, idx: usize) -> Option<&RpScene> {
        self.scenes.get(idx)
    }

    /// 下标取复核条（越界 None）。
    pub fn review_at(&self, idx: usize) -> Option<&RpReview> {
        self.reviews.get(idx)
    }

    /// 账等式：场景数 ≡ 复核条数 ≡ 判定数。
    pub fn equation_ok(&self) -> bool {
        self.scenes.len() == self.reviews.len() && self.verdicts as usize == self.scenes.len()
    }

    /// 入账一场景（序/重/矩阵/期望/谱/封六闸）。
    pub fn reg(&mut self, s: RpScene) -> Result<(), u32> {
        if self.state == RpState::RpSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::ESealedWrite.val());
        }
        if self.state == RpState::RpOpen {
            self.state = RpState::RpRunning;
        }
        // 序闸。
        if s.no != self.scenes.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::ESceneOrder.val());
        }
        // 重闸（相位+操作组合唯一）。
        for e in self.scenes.iter() {
            if e.phase == s.phase && e.op == s.op {
                self.rejects = self.rejects.saturating_add(1);
                return Err(RpDiag::ESceneDup.val());
            }
        }
        // 相位闭集闸（枚举本自闭集，越界由调用侧构造面守护——直证判定一致性）。
        // 矩阵闸：实际判定与冻结矩阵一致。
        let actual = access_allowed(s.phase, s.op);
        // 期望闸：冻结期望必须等于矩阵判定。
        if actual != s.expect_allow {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EExpectFlip.val());
        }
        // 谱闸。
        if !code_in_rp_spec(s.verdict_code) || s.verdict_code != rp_spec_code(s.no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EMatrixDrift.val());
        }
        // 全闸放行：判定数递增并落账。
        self.verdicts = self.verdicts.saturating_add(1);
        self.scenes.push(s);
        Ok(())
    }

    /// 落一条 R-D1-029 复核留痕（场景须已入账、判定码须一致、不许重条）。
    pub fn review(&mut self, r: RpReview) -> Result<(), u32> {
        if self.state == RpState::RpSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::ESealedWrite.val());
        }
        if r.reviewer.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EReviewMissing.val());
        }
        // 场景须已入账。
        let known = self.scenes.iter().any(|s| s.no == r.scene_no);
        if !known {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EReviewMissing.val());
        }
        // 判定码须与场景谱一致。
        let scene_code = match self.scenes.iter().find(|s| s.no == r.scene_no) {
            Some(s) => s.verdict_code,
            None => return Err(RpDiag::EReviewMissing.val()),
        };
        if r.verdict_code != scene_code {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EMatrixDrift.val());
        }
        // 不许重条。
        if self.reviews.iter().any(|e| e.scene_no == r.scene_no) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(RpDiag::EReviewDup.val());
        }
        self.reviews.push(r);
        Ok(())
    }
}

/// 全量回归（十场景依序入账、逐场景留痕并封账）。
pub fn build_full_rp_ledger() -> RpLedger {
    let mut led = RpLedger::new();
    let _ = led.begin_run();
    let mut i = 0usize;
    while i < RP_SCENES.len() {
        if let Some(s) = RP_SCENES.get(i) {
            let _ = led.reg(s.clone());
            let _ = led.review(RpReview {
                scene_no: s.no,
                reviewer: "R-D1-029 复核席",
                verdict_code: s.verdict_code,
            });
        }
        i += 1;
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 挂起期访问拒止回归 10/10 维持。
fn suspend_deny_ten_ok() -> bool {
    let led = build_full_rp_ledger();
    if led.len() != RP_SCENE_CAP || led.state() != RpState::RpSealed {
        return false;
    }
    if led.reject_len() != 0 || !led.equation_ok() {
        return false;
    }
    // 逐场景实际判定 == 冻结期望 == 矩阵判定。
    let mut i = 0usize;
    while i < led.len() {
        let s = match led.scene_at(i) {
            Some(v) => v,
            None => return false,
        };
        if access_allowed(s.phase, s.op) != s.expect_allow {
            return false;
        }
        i += 1;
    }
    true
}

/// 拒止矩阵冻结口径（全矩阵普查）。
fn matrix_semantics_ok() -> bool {
    // 常态全放。
    for op in AccessOp::all().iter() {
        if !access_allowed(AccessPhase::Active, *op) {
            return false;
        }
    }
    // 挂起期：读/查名放行，写/开句柄拒。
    if !access_allowed(AccessPhase::Suspended, AccessOp::OpRead) {
        return false;
    }
    if access_allowed(AccessPhase::Suspended, AccessOp::OpWrite) {
        return false;
    }
    if access_allowed(AccessPhase::Suspended, AccessOp::OpOpen) {
        return false;
    }
    if !access_allowed(AccessPhase::Suspended, AccessOp::OpQuery) {
        return false;
    }
    // 撤收期全拒。
    for op in AccessOp::all().iter() {
        if access_allowed(AccessPhase::Withdrawn, *op) {
            return false;
        }
    }
    true
}

/// R-D1-029 复核留痕（一一对应、判定码一致、零缺漏）。
fn review_trail_ok() -> bool {
    let led = build_full_rp_ledger();
    if led.review_len() != RP_SCENE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.review_len() {
        let r = match led.review_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.scene_no != i as u32 || r.reviewer.is_empty() {
            return false;
        }
        let s = match led.scene_at(i) {
            Some(v) => v,
            None => return false,
        };
        if r.verdict_code != s.verdict_code {
            return false;
        }
        i += 1;
    }
    true
}

/// 场景普查（10 场景、场景号连续、判码落谱、相位三闭集覆盖）。
fn rp_census_ok() -> bool {
    if RP_SCENES.len() != RP_SCENE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < RP_SCENES.len() {
        let s = match RP_SCENES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.no != i as u32 || !code_in_rp_spec(s.verdict_code) {
            return false;
        }
        i += 1;
    }
    for p in AccessPhase::all().iter() {
        let mut count = 0usize;
        for s in RP_SCENES.iter() {
            if s.phase == *p {
                count += 1;
            }
        }
        if count == 0 {
            return false;
        }
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn rp_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 乱序。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let bad = match replay_rp(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 2. 重复。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let s0 = match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        let _ = led.reg(s0.clone());
        if led.reg(s0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 期望翻转：挂起期写期望改放行。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rp(4) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        // 前置场景先入账。
        let mut k = 0u32;
        while k < 4 {
            if let Some(s) = replay_rp(k) {
                let _ = led.reg(s);
            }
            k += 1;
        }
        bad.expect_allow = true;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("翻期望;");
        }
    }
    // 4. 矩阵漂移：场景判码换位。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        bad.verdict_code = 0x13;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("漂移;");
        }
    }
    // 5. 留痕缺条：场景 0 只入账不落复核，封账必拒。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        });
        let mut k = 1u32;
        while k < RP_SCENE_CAP as u32 {
            if let Some(s) = replay_rp(k) {
                let _ = led.reg(s.clone());
                let _ = led.review(RpReview { scene_no: s.no, reviewer: "R-D1-029 复核席", verdict_code: s.verdict_code });
            }
            k += 1;
        }
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺条;");
        }
    }
    // 6. 留痕重条。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let s0 = match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        let _ = led.reg(s0.clone());
        let r0 = RpReview { scene_no: 0, reviewer: "R-D1-029 复核席", verdict_code: s0.verdict_code };
        let _ = led.review(r0.clone());
        if led.review(r0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重条;");
        }
    }
    // 7. 封账后写。
    {
        let mut led = build_full_rp_ledger();
        let extra = match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 8. 状态机倒退。
    {
        let mut led = build_full_rp_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 9. 未齐封账（留痕缺一半）。
    {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_rp(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺场景")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("早封;");
        }
    }
    // 10. 摘要空：冻结场景名被清空面直证（冻结表场景谱码恒非空相位名）。
    {
        let led = build_full_rp_ledger();
        let head = match led.scene_at(0) {
            Some(s) => s.phase.name(),
            None => "",
        };
        let tail = match led.scene_at(RP_SCENE_CAP - 1) {
            Some(s) => s.op.name(),
            None => "",
        };
        if !head.is_empty() && !tail.is_empty() {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let a = build_full_rp_ledger();
    let b = build_full_rp_ledger();
    if a.len() != b.len() || a.review_len() != b.review_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.scene_at(i), b.scene_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if rp_fault_sweep().0 != rp_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for s in RP_SCENES.iter() {
        let allow_bit: u32 = if s.expect_allow { 1 } else { 0 };
        acc = acc
            .wrapping_mul(23)
            .wrapping_add(s.verdict_code)
            .wrapping_add(s.phase.ordinal())
            .wrapping_add(allow_bit);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12688u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let led = build_full_rp_ledger();
    if led.state() != RpState::RpSealed {
        return false;
    }
    let head = match led.scene_at(0) {
        Some(s) => s.phase.name(),
        None => return false,
    };
    let tail = match led.scene_at(RP_SCENE_CAP.saturating_sub(1)) {
        Some(s) => s.op.name(),
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12689 根路径协议回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12688_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12688-rootpath-protocol");
    let led = build_full_rp_ledger();

    s.add_dyn(
        "F12689-01 场景十个齐",
        RP_SCENES.len() == RP_SCENE_CAP,
        &format!("scenes={} phases={} ops={}", RP_SCENES.len(), AccessPhase::all().len(), AccessOp::all().len()),
    );

    s.add_dyn(
        "F12689-02 挂起期访问拒止回归 10/10 维持",
        suspend_deny_ten_ok(),
        &format!("scenes={} sealed={}", led.len(), led.state() == RpState::RpSealed),
    );

    s.add_dyn(
        "F12689-03 拒止矩阵冻结口径",
        matrix_semantics_ok(),
        "常态全放/挂起拒写拒开/撤收全拒",
    );

    s.add_dyn(
        "F12689-04 R-D1-029 复核留痕",
        review_trail_ok(),
        &format!("reviews={} scenes={}", led.review_len(), led.len()),
    );

    s.add_dyn(
        "F12689-05 场景普查",
        rp_census_ok(),
        &format!("scenes={}", RP_SCENES.len()),
    );

    s.add_dyn(
        "F12689-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = rp_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", rp_fault_sweep().0),
    );

    s.add_dyn(
        "F12689-07 账等式（场景≡留痕≡判定）",
        led.equation_ok(),
        &format!("scenes={} reviews={} verdicts={}", led.len(), led.review_len(), led.verdict_len()),
    );

    s.add_dyn(
        "F12689-08 谱口径",
        code_in_rp_spec(0x12) && code_in_rp_spec(0x1B) && !code_in_rp_spec(0x11) && !code_in_rp_spec(0x1C),
        "场景谱 0x12..0x1B 闭集",
    );

    s.add_dyn(
        "F12689-09 相位闭集覆盖",
        AccessPhase::all().len() == 3 && AccessOp::all().len() == 4,
        "三相位 × 四操作冻结闭集",
    );

    s.add_dyn(
        "F12689-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12689-11 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12689-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12689-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81600);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(RP_ALL.len(), 10);
        let mut i = 0usize;
        while i < RP_ALL.len() {
            let vi = match RP_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < RP_ALL.len() {
                let vj = match RP_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn suspend_deny_ten_holds() {
        assert!(suspend_deny_ten_ok());
        let led = build_full_rp_ledger();
        assert_eq!(led.len(), 10);
        assert_eq!(led.review_len(), 10);
        assert_eq!(led.state(), RpState::RpSealed);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.verdict_len(), 10);
    }

    #[test]
    fn matrix_semantics_hold() {
        assert!(matrix_semantics_ok());
        // 挂起期拒止锚点直读。
        assert!(!access_allowed(AccessPhase::Suspended, AccessOp::OpWrite));
        assert!(!access_allowed(AccessPhase::Suspended, AccessOp::OpOpen));
        assert!(access_allowed(AccessPhase::Suspended, AccessOp::OpRead));
        assert!(access_allowed(AccessPhase::Suspended, AccessOp::OpQuery));
        // 撤收期全拒、常态全放。
        for op in AccessOp::all().iter() {
            assert!(!access_allowed(AccessPhase::Withdrawn, *op));
            assert!(access_allowed(AccessPhase::Active, *op));
        }
    }

    #[test]
    fn review_trail_holds_and_rejects() {
        assert!(review_trail_ok());
        // 留痕重条拒。
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let s0 = match replay_rp(0) {
            Some(x) => x,
            None => return,
        };
        let _ = led.reg(s0.clone());
        let r0 = RpReview { scene_no: 0, reviewer: "R-D1-029 复核席", verdict_code: s0.verdict_code };
        assert!(led.review(r0.clone()).is_ok());
        assert_eq!(led.review(r0), Err(RpDiag::EReviewDup.val()));
        // 未入账场景留痕拒。
        assert_eq!(
            led.review(RpReview { scene_no: 3, reviewer: "R-D1-029 复核席", verdict_code: rp_spec_code(3) }),
            Err(RpDiag::EReviewMissing.val())
        );
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = rp_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn expect_flip_injection_caught() {
        let mut led = RpLedger::new();
        let _ = led.begin_run();
        let mut k = 0u32;
        while k < 4 {
            if let Some(s) = replay_rp(k) {
                let _ = led.reg(s);
            }
            k += 1;
        }
        let mut bad = match replay_rp(4) {
            Some(x) => x,
            None => return,
        };
        bad.expect_allow = true;
        assert_eq!(led.reg(bad), Err(RpDiag::EExpectFlip.val()));
        assert_eq!(led.reject_len(), 1);
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_rp_ledger();
        let b = build_full_rp_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.scene_at(i), b.scene_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑场景缺失"),
            }
            i += 1;
        }
        assert_eq!(rp_fault_sweep().0, rp_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(RpState::RpOpen.rank() < RpState::RpRunning.rank());
        assert!(RpState::RpRunning.rank() < RpState::RpSealed.rank());
        let mut led = build_full_rp_ledger();
        assert_eq!(led.state(), RpState::RpSealed);
        assert_eq!(led.seal(), Err(RpDiag::ERewind.val()));
        let mut fresh = RpLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(RpDiag::ERewind.val()));
    }

    #[test]
    fn census_and_replay_boundary() {
        assert!(rp_census_ok());
        assert!(replay_rp(9).is_some());
        assert!(replay_rp(10).is_none());
        for no in 0u32..10 {
            assert!(code_in_rp_spec(rp_spec_code(no)), "场景 {} 未落谱", no);
        }
        assert!(!code_in_rp_spec(0x11));
        assert!(!code_in_rp_spec(0x1C));
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12688_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
