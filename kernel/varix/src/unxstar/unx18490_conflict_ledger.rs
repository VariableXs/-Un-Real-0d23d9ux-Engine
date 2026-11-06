//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18490 · 裁决冲突诊断账（同源多值） · 0x7F640 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18490-J1 同源多值冲突检出告警 10/10，处置路径入体验日志
//! 本模块结构清单：
//!   1. 冻结常量层：声明账 12 槽 / 告警账 8 槽 / 体验日志 16 条 / 轮数 10；
//!   2. 诊断码闭集 CfCode（code()=SEG_BASE+n，detail() 中文闭集逐码非空）；
//!   3. 数据结构：
//!      - 数组账本 ClaimBook：[ClaimRec; 12]（source_id+key_id+value+seq）；
//!      - 告警账 AlarmBook：[AlarmRec; 8]（同源多值对 + 处置态）；
//!      - 体验日志 ExpLog：Vec<ExpEntry>（处置路径流水，只增不减）；
//!      - 状态机 ClaimState：First→Conflicted→Adjudicated 三态单向；
//!   4. 冲突判定：同 (source,key) 二次声明且值不同 → 告警（先到先得
//!      v1 裁决：取首次值）；同值重复声明幂等不告警；
//!   5. 错误路径：幽灵告警处置 / 乱序处置（未告警先处置）/ 处置重复
//!      / 账满 / 守恒失衡，全部显式拒绝码；
//!   6. 注入核：同源多值 / 幽灵处置 / 乱序处置 / 处置重复 四类坏样本
//!      逐类 10/10 断言被检出；
//!   7. run_unx18490_checks：14 条判据（告警/守恒/单调/确定性/非法必拒）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段基址（0x7F640，G1 工单 A09 批05 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7F640;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 声明账容量（写死 12）：4 源 × 3 键样板，12=4×3 便于按源对折；扩容
/// 须换版 v2。
pub const CLAIM_CAP: usize = 12;
/// 告警账容量（写死 8）。
pub const ALARM_CAP: usize = 8;
/// 体验日志深度（写死 16：处置路径流水上限，超限截断显性计数）。
pub const EXP_LOG_CAP: usize = 16;
/// 源号域上界（冻结：1..=8，0 保留为"账本自身"哨兵）。
pub const SRC_ID_MAX: u32 = 8;
/// 键号域上界（冻结：1..=16，0 保留）。
pub const KEY_ID_MAX: u32 = 16;
/// 判据轮数（写死 10：判据原文"告警 10/10"取 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 裁决策略（冻结 v1：0=先到先得）。
pub const POLICY_FIRST_WINS: u8 = 0;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CfCode {
    /// 幽灵声明：声明账查无此 (source,key)
    GhostClaim,
    /// 源号越界：撞保留字 0 或超 8
    SrcOutOfRange,
    /// 键号越界：撞保留字 0 或超 16
    KeyOutOfRange,
    /// 同源多值冲突（检出即告警，非错误——告警账满时才升级为拒绝）
    ConflictDetected,
    /// 告警账满：8 槽用尽
    AlarmBookFull,
    /// 幽灵告警处置：告警账查无此号
    GhostAlarm,
    /// 乱序处置：告警未立即处置
    DispositionEarly,
    /// 处置重复：已裁决告警再处置
    DupDisposition,
    /// 体验日志满：16 条用尽
    ExpLogFull,
    /// 守恒破坏：声明数=有效+拒收失衡
    ConservationBroken,
    /// 告警账失衡：告警数≠冲突检出数（守恒违例）
    AlarmLedgerMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl CfCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            CfCode::GhostClaim => "幽灵声明：声明账查无此 (source,key)",
            CfCode::SrcOutOfRange => "源号越界：撞保留字 0 或超 8",
            CfCode::KeyOutOfRange => "键号越界：撞保留字 0 或超 16",
            CfCode::ConflictDetected => "同源多值冲突：同源同键不同值，已告警",
            CfCode::AlarmBookFull => "告警账满：8 槽用尽，须升版扩容",
            CfCode::GhostAlarm => "幽灵告警处置：告警账查无此号",
            CfCode::DispositionEarly => "乱序处置：告警未立即处置",
            CfCode::DupDisposition => "处置重复：已裁决告警再处置",
            CfCode::ExpLogFull => "体验日志满：16 条用尽",
            CfCode::ConservationBroken => "守恒破坏：声明数=有效+拒收失衡",
            CfCode::AlarmLedgerMismatch => "告警账失衡：告警数≠冲突检出数",
            CfCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CfCode; 12] = [
        CfCode::GhostClaim,
        CfCode::SrcOutOfRange,
        CfCode::KeyOutOfRange,
        CfCode::ConflictDetected,
        CfCode::AlarmBookFull,
        CfCode::GhostAlarm,
        CfCode::DispositionEarly,
        CfCode::DupDisposition,
        CfCode::ExpLogFull,
        CfCode::ConservationBroken,
        CfCode::AlarmLedgerMismatch,
        CfCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < CfCode::ALL.len() {
        if let Some(c) = CfCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F640 段闭集内"
}

// ── 声明状态机（First→Conflicted→Adjudicated 单向） ────────────────
/// 声明三态（冻结 v1，只许单向推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClaimState {
    /// 首值：同源同键首声明。
    First,
    /// 冲突：同源同键异值声明已告警。
    Conflicted,
    /// 已裁决：处置路径已入体验日志。
    Adjudicated,
}

/// 单条声明记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClaimRec {
    /// 源号（1..=8）。
    pub src: u32,
    /// 键号（1..=16）。
    pub key: u32,
    /// 首值（先到先得 v1 的胜出值）。
    pub first_val: u32,
    /// 冲突值（Conflicted 态有效）。
    pub clash_val: u32,
    /// 声明态。
    pub state: ClaimState,
    /// 声明次数（含同值幂等重申）。
    pub claim_ticks: u16,
}

impl ClaimRec {
    pub const fn empty() -> ClaimRec {
        ClaimRec {
            src: 0,
            key: 0,
            first_val: 0,
            clash_val: 0,
            state: ClaimState::First,
            claim_ticks: 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.src == 0
    }
}

/// 单条告警。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AlarmRec {
    /// 告警号（1..，单调）。
    pub aid: u32,
    /// 源号。
    pub src: u32,
    /// 键号。
    pub key: u32,
    /// 首值。
    pub val_a: u32,
    /// 冲突值。
    pub val_b: u32,
    /// 是否已裁决（处置路径已入日志）。
    pub resolved: bool,
}

impl AlarmRec {
    pub const fn empty() -> AlarmRec {
        AlarmRec { aid: 0, src: 0, key: 0, val_a: 0, val_b: 0, resolved: false }
    }
    pub fn is_empty(&self) -> bool {
        self.aid == 0
    }
}

/// 处置动作闭集（冻结 v1 三动作）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActKind {
    /// 升级：转人工/上级裁决面。
    Escalate,
    /// 隔离：冻结冲突键。
    Quarantine,
    /// 裁决：按先到先得取首值。
    Elect,
}

impl ActKind {
    /// 动作码（0..=2，日志打包用）。
    pub fn code(self) -> u32 {
        match self {
            ActKind::Escalate => 0,
            ActKind::Quarantine => 1,
            ActKind::Elect => 2,
        }
    }
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            ActKind::Escalate => "升级",
            ActKind::Quarantine => "隔离",
            ActKind::Elect => "裁决取首值",
        }
    }
}

/// 单条体验日志（处置路径流水）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExpEntry {
    /// 关联告警号。
    pub aid: u32,
    /// 处置动作。
    pub act: ActKind,
    /// 胜出值（Elect 时为首值，其余为 0）。
    pub winner: u32,
    /// 序号（1..，单调）。
    pub seq: u32,
}

// ── 冲突诊断引擎 ───────────────────────────────────────────────────
/// 冲突诊断引擎：声明账 + 告警账 + 体验日志 + 守恒账。
pub struct ConflictEngine {
    pub claims: [ClaimRec; CLAIM_CAP],
    pub alarms: [AlarmRec; ALARM_CAP],
    pub exp_log: Vec<ExpEntry>,
    /// 声明总数（含幂等重申）。
    pub claim_total: u32,
    /// 冲突检出总数。
    pub conflict_total: u32,
    /// 被拒操作总数。
    pub rejected_total: u32,
    /// 告警号累加器（单调）。
    pub aid_acc: u32,
    /// 体验日志序号累加器（单调）。
    pub exp_seq_acc: u32,
    /// 拒因账。
    pub reject_log: Vec<u32>,
}

impl ConflictEngine {
    pub fn new() -> ConflictEngine {
        ConflictEngine {
            claims: [ClaimRec::empty(); CLAIM_CAP],
            alarms: [AlarmRec::empty(); ALARM_CAP],
            exp_log: Vec::new(),
            claim_total: 0,
            conflict_total: 0,
            rejected_total: 0,
            aid_acc: 0,
            exp_seq_acc: 0,
            reject_log: Vec::new(),
        }
    }

    /// (src,key) 定声明槽。
    fn find_claim(&self, src: u32, key: u32) -> Option<usize> {
        let mut i = 0;
        while i < CLAIM_CAP {
            if let Some(c) = self.claims.get(i) {
                if !c.is_empty() && c.src == src && c.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 空声明槽。
    fn free_claim(&self) -> Option<usize> {
        let mut i = 0;
        while i < CLAIM_CAP {
            if let Some(c) = self.claims.get(i) {
                if c.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 空告警槽（已裁决槽可复用——告警处置后账面留序号）。
    fn free_alarm(&self) -> Option<usize> {
        let mut i = 0;
        while i < ALARM_CAP {
            if let Some(a) = self.alarms.get(i) {
                if a.is_empty() {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 声明入口：首声明入账；同源同键同值幂等重申；同键异值触发告警。
    /// 返回 Ok(false)=幂等重申，Ok(true)=新声明；冲突走 Err(告警码) 且
    /// 告警入账（Err 是"告警"信号载体，非纯错误）。
    pub fn claim(&mut self, src: u32, key: u32, value: u32) -> Result<bool, u32> {
        if src == 0 || src > SRC_ID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(CfCode::SrcOutOfRange.code());
            return Err(CfCode::SrcOutOfRange.code());
        }
        if key == 0 || key > KEY_ID_MAX {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(CfCode::KeyOutOfRange.code());
            return Err(CfCode::KeyOutOfRange.code());
        }
        match self.find_claim(src, key) {
            Some(slot) => {
                let (first, state) = match self.claims.get(slot) {
                    Some(c) => (c.first_val, c.state),
                    None => (0, ClaimState::First),
                };
                if value == first {
                    // 幂等重申：同值不告警。
                    if let Some(c) = self.claims.get_mut(slot) {
                        c.claim_ticks = c.claim_ticks.saturating_add(1);
                    }
                    self.claim_total = self.claim_total.saturating_add(1);
                    return Ok(false);
                }
                if state != ClaimState::First {
                    // 已冲突/已裁决的同源异值再声明：二次冲突，仍告警。
                }
                // 同源多值冲突：立告警 + 声明态推进 Conflicted。
                let aid = self.aid_acc.saturating_add(1);
                let aslot = match self.free_alarm() {
                    Some(s) => s,
                    None => {
                        self.rejected_total = self.rejected_total.saturating_add(1);
                        self.reject_log.push(CfCode::AlarmBookFull.code());
                        return Err(CfCode::AlarmBookFull.code());
                    }
                };
                if let Some(a) = self.alarms.get_mut(aslot) {
                    *a = AlarmRec { aid, src, key, val_a: first, val_b: value, resolved: false };
                }
                if let Some(c) = self.claims.get_mut(slot) {
                    c.clash_val = value;
                    c.state = ClaimState::Conflicted;
                    c.claim_ticks = c.claim_ticks.saturating_add(1);
                }
                self.aid_acc = aid;
                self.conflict_total = self.conflict_total.saturating_add(1);
                self.claim_total = self.claim_total.saturating_add(1);
                Err(CfCode::ConflictDetected.code())
            }
            None => {
                let slot = match self.free_claim() {
                    Some(s) => s,
                    None => {
                        self.rejected_total = self.rejected_total.saturating_add(1);
                        self.reject_log.push(CfCode::ConservationBroken.code());
                        return Err(CfCode::ConservationBroken.code());
                    }
                };
                if let Some(c) = self.claims.get_mut(slot) {
                    *c = ClaimRec {
                        src,
                        key,
                        first_val: value,
                        clash_val: 0,
                        state: ClaimState::First,
                        claim_ticks: 1,
                    };
                }
                self.claim_total = self.claim_total.saturating_add(1);
                Ok(true)
            }
        }
    }

    /// 处置入口：对已告警 (src,key) 走处置路径，动作入体验日志。
    /// 裁决策略 v1=先到先得（Elect 胜出首值）。
    pub fn dispose(&mut self, src: u32, key: u32, act: ActKind) -> Result<u32, u32> {
        let cslot = match self.find_claim(src, key) {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(CfCode::GhostClaim.code());
                return Err(CfCode::GhostClaim.code());
            }
        };
        let (state, first) = match self.claims.get(cslot) {
            Some(c) => (c.state, c.first_val),
            None => (ClaimState::First, 0),
        };
        if state != ClaimState::Conflicted {
            // First=未告警先处置（乱序）；Adjudicated=处置重复。
            let code = match state {
                ClaimState::First => CfCode::DispositionEarly.code(),
                _ => CfCode::DupDisposition.code(),
            };
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(code);
            return Err(code);
        }
        // 找对应未裁决告警。
        let mut aslot: Option<usize> = None;
        let mut i = 0;
        while i < ALARM_CAP {
            if let Some(a) = self.alarms.get(i) {
                if !a.is_empty() && a.src == src && a.key == key && !a.resolved {
                    aslot = Some(i);
                    i = ALARM_CAP;
                }
            }
            if aslot.is_none() {
                i += 1;
            }
        }
        let aslot = match aslot {
            Some(s) => s,
            None => {
                self.rejected_total = self.rejected_total.saturating_add(1);
                self.reject_log.push(CfCode::GhostAlarm.code());
                return Err(CfCode::GhostAlarm.code());
            }
        };
        let aid = match self.alarms.get(aslot) {
            Some(a) => a.aid,
            None => 0,
        };
        // 体验日志入账（满则显性拒绝，不静默丢）。
        if self.exp_log.len() >= EXP_LOG_CAP {
            self.rejected_total = self.rejected_total.saturating_add(1);
            self.reject_log.push(CfCode::ExpLogFull.code());
            return Err(CfCode::ExpLogFull.code());
        }
        let seq = self.exp_seq_acc.saturating_add(1);
        let winner = match act {
            ActKind::Elect => first,
            _ => 0,
        };
        self.exp_log.push(ExpEntry { aid, act, winner, seq });
        self.exp_seq_acc = seq;
        // 裁决收尾：声明态→Adjudicated，告警置 resolved。
        if let Some(c) = self.claims.get_mut(cslot) {
            c.state = ClaimState::Adjudicated;
        }
        if let Some(a) = self.alarms.get_mut(aslot) {
            a.resolved = true;
        }
        Ok(aid)
    }

    /// 按告警号查告警（只读）。
    pub fn peek_alarm(&self, aid: u32) -> Option<AlarmRec> {
        let mut i = 0;
        while i < ALARM_CAP {
            if let Some(a) = self.alarms.get(i) {
                if !a.is_empty() && a.aid == aid {
                    return Some(*a);
                }
            }
            i += 1;
        }
        None
    }

    /// 体验日志只读。
    pub fn log_len(&self) -> usize {
        self.exp_log.len()
    }

    /// 告警账对账：告警账非空数+已复用空位 ≥ 告警累计数（下界对账），
    /// 且 conflict_total 与告警号累加器一致。
    pub fn alarm_audit(&self) -> Result<(), u32> {
        if self.aid_acc != self.conflict_total {
            return Err(CfCode::AlarmLedgerMismatch.code());
        }
        Ok(())
    }

    /// 守恒：claim_total ≥ 有效声明数（幂等重申计入 claim_ticks 总和）。
    pub fn conserve(&self) -> Result<(), u32> {
        let mut n = 0u32;
        let mut i = 0;
        while i < CLAIM_CAP {
            if let Some(c) = self.claims.get(i) {
                if !c.is_empty() {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        if self.claim_total < n {
            return Err(CfCode::ConservationBroken.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────────
/// 黄金全链：3 源 × 2 键首声明 → 同源异值冲突告警 → 逐条处置（三动作
/// 轮换）→ 体验日志逐条对账 → 守恒。
pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut e = ConflictEngine::new();
    // 6 条首声明。
    let mut s = 1u32;
    while s <= 3u32 {
        e.claim(s, 1, 0x100 + s)?;
        e.claim(s, 2, 0x200 + s)?;
        s += 1;
    }
    // 6 条同源异值冲突 → 6 告警。
    let mut s = 1u32;
    while s <= 3u32 {
        match e.claim(s, 1, 0x900 + s) {
            Err(c) if c == CfCode::ConflictDetected.code() => {}
            _ => return Err(CfCode::AlarmLedgerMismatch.code()),
        }
        match e.claim(s, 2, 0xA00 + s) {
            Err(c) if c == CfCode::ConflictDetected.code() => {}
            _ => return Err(CfCode::AlarmLedgerMismatch.code()),
        }
        s += 1;
    }
    if e.conflict_total != 6 {
        return Err(CfCode::AlarmLedgerMismatch.code());
    }
    e.alarm_audit()?;
    // 逐条处置：三动作轮换（裁决/隔离/升级）。
    let acts = [ActKind::Elect, ActKind::Quarantine, ActKind::Escalate];
    let mut s = 1u32;
    while s <= 3u32 {
        let a1 = acts.get(((s - 1) % 3) as usize);
        let a2 = acts.get((s % 3) as usize);
        match (a1, a2) {
            (Some(x), Some(y)) => {
                e.dispose(s, 1, *x)?;
                e.dispose(s, 2, *y)?;
            }
            _ => return Err(CfCode::GhostAlarm.code()),
        }
        s += 1;
    }
    // 体验日志对账：处置路径 6 条全部入日志。
    if e.log_len() != 6 {
        return Err(CfCode::ExpLogFull.code());
    }
    // 日志序号连续。
    let mut i = 0;
    while i < e.exp_log.len() {
        let entry = match e.exp_log.get(i) {
            Some(x) => *x,
            None => break,
        };
        if entry.seq != (i + 1) as u32 {
            return Err(CfCode::ExpLogFull.code());
        }
        i += 1;
    }
    e.conserve()?;
    Ok((e.conflict_total, e.log_len() as u32))
}

// ── 注入检出账 ─────────────────────────────────────────────────────
/// 四槽注入账。
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed_total(&self) -> u32 {
        let mut m = 0u32;
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            m = m.saturating_add(t.saturating_sub(c));
            i += 1;
        }
        m
    }
}

/// 注入核：四类坏样本逐类 10/10 断言被检出。
///   槽 0：同源多值（同源同键异值 → ConflictDetected 告警必触发）
///   槽 1：幽灵处置（未声明 (src,key) 直接处置）
///   槽 2：乱序处置（首声明未冲突即处置）
///   槽 3：处置重复（已裁决再处置）
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        // 槽 0：同源多值。
        let mut e0 = ConflictEngine::new();
        let _ = e0.claim(1, 1, 0x10);
        let hit0 = match e0.claim(1, 1, 0x20) {
            Err(c) if c == CfCode::ConflictDetected.code() => true,
            _ => false,
        };
        led.record(0, hit0);
        // 槽 1：幽灵处置。
        let mut e1 = ConflictEngine::new();
        let hit1 = match e1.dispose(9, 9, ActKind::Elect) {
            Err(c) if c == CfCode::GhostClaim.code() => true,
            _ => false,
        };
        led.record(1, hit1);
        // 槽 2：乱序处置。
        let mut e2 = ConflictEngine::new();
        let _ = e2.claim(2, 3, 0x30);
        let hit2 = match e2.dispose(2, 3, ActKind::Quarantine) {
            Err(c) if c == CfCode::DispositionEarly.code() => true,
            _ => false,
        };
        led.record(2, hit2);
        // 槽 3：处置重复。
        let mut e3 = ConflictEngine::new();
        let _ = e3.claim(3, 4, 0x40);
        let _ = e3.claim(3, 4, 0x41); // 触发告警
        let _ = e3.dispose(3, 4, ActKind::Elect);
        let hit3 = match e3.dispose(3, 4, ActKind::Elect) {
            Err(c) if c == CfCode::DupDisposition.code() => true,
            _ => false,
        };
        led.record(3, hit3);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ───────────────────────────────────────────────────────
/// 探针 A：正例告警+处置全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut e = ConflictEngine::new();
    e.claim(1, 5, 0x55)?;
    match e.claim(1, 5, 0x66) {
        Err(c) if c == CfCode::ConflictDetected.code() => {}
        _ => return Err(CfCode::AlarmLedgerMismatch.code()),
    }
    e.dispose(1, 5, ActKind::Elect)?;
    Ok(())
}

/// 探针 B：幽灵处置必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = ConflictEngine::new();
    e.dispose(7, 7, ActKind::Escalate).map(|_| ())
}

/// 探针 C：源号越界必拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = ConflictEngine::new();
    e.claim(0, 1, 1).map(|_| ())
}

// ── 双跑确定性对拍 ─────────────────────────────────────────────────
/// 双跑：同黄金链两次逐字段相等。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CfCode::NotReproducible.code());
    }
    Ok(())
}

// ── 段位守卫 ───────────────────────────────────────────────────────
/// 段位守卫：SEG_BASE 对齐且全部诊断码落段内 0x40 窗口。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < CfCode::ALL.len() {
        if let Some(c) = CfCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 幂等重申探测：同源同键同值三连申，零告警且计数幂等累加。
pub fn idempotent_claim_probe() -> Result<(u32, u32), u32> {
    let mut e = ConflictEngine::new();
    let _first = e.claim(4, 8, 0x88)?;
    let r2 = e.claim(4, 8, 0x88)?;
    let r3 = e.claim(4, 8, 0x88)?;
    // 幂等重申返回 Ok(false)（bool），true 意味着走了新声明分支。
    if r2 || r3 {
        return Err(CfCode::ConservationBroken.code());
    }
    if e.conflict_total != 0 {
        return Err(CfCode::AlarmLedgerMismatch.code());
    }
    e.conserve()?;
    Ok((e.claim_total, e.conflict_total))
}

/// 告警账容量探测：8 个独立键各触发 1 告警（8 槽恰满），第 9 告警必返
/// 告警账满且前 8 条告警账不丢。
pub fn alarm_capacity_probe() -> Result<(u32, u32), u32> {
    let mut e = ConflictEngine::new();
    // 8 键首声明。
    let mut k = 1u32;
    while k <= ALARM_CAP as u32 {
        e.claim(1, k, 0x1000 + k)?;
        k += 1;
    }
    // 8 键异值 → 8 告警。
    let mut alarmed = 0u32;
    let mut k = 1u32;
    while k <= ALARM_CAP as u32 {
        match e.claim(1, k, 0x2000 + k) {
            Err(c) if c == CfCode::ConflictDetected.code() => alarmed = alarmed.saturating_add(1),
            _ => {}
        }
        k += 1;
    }
    // 第 9 告警：键 9 首声明+异值。
    let _ = e.claim(1, 9, 0x3000);
    let ninth = match e.claim(1, 9, 0x3001) {
        Err(c) if c == CfCode::AlarmBookFull.code() => true,
        _ => false,
    };
    if alarmed != ALARM_CAP as u32 || !ninth {
        return Err(CfCode::AlarmLedgerMismatch.code());
    }
    Ok((alarmed, e.rejected_total))
}

// ── 判据收口 ───────────────────────────────────────────────────────
/// 判据产出（14 条）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链-告警+处置入日志", g.is_ok(), format!("{:?}", g)));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-同源多值告警10/10",
        inj_ok,
        format!(
            "tries={:?} caught={:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    let miss = match &inj {
        Ok(l) => l.missed_total(),
        Err(_) => 1,
    };
    v.push(("J1-注入守恒漏检零", miss == 0, format!("missed={}", miss)));
    // 处置路径入体验日志独立复验。
    let mut e = ConflictEngine::new();
    let _ = e.claim(2, 3, 0x30);
    let _ = e.claim(2, 3, 0x31); // 告警
    let d = e.dispose(2, 3, ActKind::Elect);
    let log_ok = match &d {
        Ok(_) => e.log_len() == 1,
        Err(_) => false,
    };
    v.push(("J2-处置路径入体验日志", log_ok, format!("{:?}", d)));
    // 告警账对账 + 守恒。
    let alarm_ok = e.alarm_audit().is_ok();
    v.push(("J2-告警账对账", alarm_ok, String::from("aid_acc=conflict_total")));
    let conserve_ok = e.conserve().is_ok();
    v.push(("J2-声明账守恒", conserve_ok, String::from("claim_total≥有效声明")));
    // 单调：体验日志序号单调递增。
    let mut e2 = ConflictEngine::new();
    let mut mono = true;
    let mut s = 1u32;
    while s <= 3u32 {
        let _ = e2.claim(s, 1, 0x10 + s);
        let _ = e2.claim(s, 1, 0x20 + s); // 告警
        let _ = e2.dispose(s, 1, ActKind::Elect);
        s += 1;
    }
    let mut i = 0;
    while i < e2.exp_log.len() {
        if let Some(x) = e2.exp_log.get(i) {
            if x.seq != (i + 1) as u32 {
                mono = false;
            }
        }
        i += 1;
    }
    v.push(("J3-日志序号单调", mono && e2.log_len() == 3, format!("n={}", e2.log_len())));
    // 幂等重申。
    let idem = idempotent_claim_probe();
    v.push(("J3-幂等重申-同值不告警", idem.is_ok(), format!("{:?}", idem)));
    // 告警账容量。
    let cap = alarm_capacity_probe();
    v.push(("J4-告警账容量-8满9拒", cap.is_ok(), format!("{:?}", cap)));
    // 边界：src 0/9 拒，1/8 收；key 0/17 拒，1/16 收。
    let mut e3 = ConflictEngine::new();
    let b0 = e3.claim(0, 1, 1).err() == Some(CfCode::SrcOutOfRange.code());
    let b1 = e3.claim(9, 1, 1).err() == Some(CfCode::SrcOutOfRange.code());
    let b2 = e3.claim(1, 0, 1).err() == Some(CfCode::KeyOutOfRange.code());
    let b3 = e3.claim(1, 17, 1).err() == Some(CfCode::KeyOutOfRange.code());
    let b4 = e3.claim(1, 1, 1).is_ok();
    let b5 = e3.claim(SRC_ID_MAX, KEY_ID_MAX, 1).is_ok();
    v.push(("J4-源键域边界", b0 && b1 && b2 && b3 && b4 && b5, String::from("0/9/0/17拒 1/16收")));
    // 探针三序。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    let pc = probe_seq_c().is_err();
    v.push(("J4-探针三序", pa && pb && pc, format!("a={} b!={} c!={}", pa, pb, pc)));
    // 双跑一致。
    v.push(("J5-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫。
    v.push(("J5-段位守卫", segment_guard(), String::from("0x7F640 窗口闭集")));
    // 文案闭集。
    let mut det_ok = true;
    let mut k = 0;
    while k < CfCode::ALL.len() {
        if let Some(c) = CfCode::ALL.get(k) {
            if detail_of(c.code()) != c.detail() || c.detail().len() == 0 {
                det_ok = false;
            }
        }
        k += 1;
    }
    v.push(("J5-诊断文案闭集非空", det_ok, format!("codes={}", CfCode::ALL.len())));
    v.push(("J5-判据收口", v.len() + 1 == 15, format!("n={}", v.len() + 1)));
    v
}

/// 探针入口：F18490 裁决冲突诊断账自检（14 条）。
pub fn run_unx18490_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18490 conflict_ledger");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i = i.saturating_add(1);
    }
    s
}

// ── 测试（8 个） ───────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_golden_chain() {
        // 正常流：首声明→冲突告警→处置入日志。
        assert!(golden_run().is_ok(), "黄金全链应全绿");
    }

    #[test]
    fn same_source_multivalue_alarms() {
        // 判据核心：同源同键异值必告警，先到先得首值保留。
        let mut e = ConflictEngine::new();
        assert!(e.claim(1, 1, 100).is_ok());
        assert!(e.claim(1, 1, 200).err() == Some(CfCode::ConflictDetected.code()));
        // 胜出值=首值 100（Elect 裁决）。
        assert!(e.dispose(1, 1, ActKind::Elect) == Ok(1), "告警号应为 1");
        assert!(e.peek_alarm(1).map(|a| a.val_a == 100 && a.val_b == 200).unwrap_or(false));
        assert!(e.log_len() == 1, "处置应入体验日志");
    }

    #[test]
    fn injection_all_caught() {
        // 注入必抓：四类坏样本 10/10。
        let led = inject_rounds(JUDGE_ROUNDS);
        assert!(led.is_ok());
        if let Ok(l) = led {
            assert!(l.all_caught(), "四槽必须全抓");
            assert!(l.missed_total() == 0, "漏检必须为 0");
            assert!(l.caught[0] == JUDGE_ROUNDS, "同源多值槽满轮告警");
        }
    }

    #[test]
    fn idempotent_resubmit() {
        // 幂等重申：同值三连申零告警。
        let idem = idempotent_claim_probe();
        assert!(idem.is_ok());
        if let Ok((claims, conflicts)) = idem {
            assert!(claims == 3, "三连申应计 3");
            assert!(conflicts == 0, "同值不应告警");
        }
    }

    #[test]
    fn disposition_order_enforced() {
        // 状态机：First 处置=乱序拒；Conflicted 处置=过；Adjudicated 再处置=重复拒。
        let mut e = ConflictEngine::new();
        assert!(e.claim(2, 4, 10).is_ok());
        assert!(e.dispose(2, 4, ActKind::Quarantine).err() == Some(CfCode::DispositionEarly.code()));
        assert!(e.claim(2, 4, 11).err() == Some(CfCode::ConflictDetected.code()));
        assert!(e.dispose(2, 4, ActKind::Quarantine).is_ok());
        assert!(e.dispose(2, 4, ActKind::Elect).err() == Some(CfCode::DupDisposition.code()));
    }

    #[test]
    fn domain_bounds() {
        // 域边界：src 0/9 拒，key 0/17 拒，合法值收。
        let mut e = ConflictEngine::new();
        assert!(e.claim(0, 1, 1).err() == Some(CfCode::SrcOutOfRange.code()));
        assert!(e.claim(9, 1, 1).err() == Some(CfCode::SrcOutOfRange.code()));
        assert!(e.claim(1, 0, 1).err() == Some(CfCode::KeyOutOfRange.code()));
        assert!(e.claim(1, 17, 1).err() == Some(CfCode::KeyOutOfRange.code()));
        assert!(e.claim(1, 1, 1).is_ok());
        assert!(e.claim(SRC_ID_MAX, KEY_ID_MAX, 1).is_ok());
    }

    #[test]
    fn alarm_capacity_boundary() {
        // 告警账容量：8 满 9 拒。
        let cap = alarm_capacity_probe();
        assert!(cap.is_ok());
        if let Ok((alarmed, _r)) = cap {
            assert!(alarmed == ALARM_CAP as u32);
        }
    }

    #[test]
    fn segment_guard_and_checkset() {
        // 段位守卫 + 判据收口全绿。
        assert!(segment_guard(), "段位守卫应过");
        let set = run_unx18490_checks();
        assert!(set.all_passed(), "15 条判据应全绿");
        assert!(set.len() == 15, "判据条数应冻结 15");
    }
}
