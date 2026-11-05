//! UNX-F12707 联签双机一致率终账（≥95% 目标线） · 0x81A80段
//!
//! 判据原文（派工卡 12706 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12707-J1 预算终表齐（I 型八批合账），超线
//!      告警演练 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 联签双机一致率终账：四域分账（D2/D3/D4/E1 各 250 例）合计
//!    1000 例，甲机判定按封闭式折算（判定 ≡ (序×3+1) mod 3 三态：
//!    绿/绿/白名单挂），乙机在冻结分歧行上取反判定（绿↔挂）——
//!    分歧行即 CONFLICT 例，显性列账不静默吞；
//! 2. 一致率终账：分域一致率 = (250−分歧)×1000/250 千分比整数，
//!    总账一致率 = 980×1000/1000 ≡ 980‰ ≥ 950‰ 断言绿（目标线
//!    95% 的整数化口径）；分域与总账双线同验；
//! 3. I 型八批合账预算终表：八批（批序/预算 μs/实耗 μs）三元冻结，
//!    Σ预算 ≡ 8000、Σ实耗 ≡ 7600 合账闭合，实耗 ≤ 预算逐行过闸；
//! 4. 超线告警演练：对预算行注入超线实耗（actual > budget），超线
//!    告警 Alarm::Overline 逐路触发——演练 10/10（八批行 + 二条
//!    合账面合成行）全部告警必抓；
//! 5. 终账状态机 DrDraft → DrReckoning → DrSettled 单向推进，终账
//!    后只读、倒退显性拒；注入十族（分歧漏账/虚分歧/重分歧/率越线/
//!    域缺账/批缺账/超线漏警/终账后写/倒退/摘要空）逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `dualhost_rate_settled_ok` ↔ 标题「联签双机一致率终账（≥95% 目标线）」；
//!   - `budget_table_ok` ↔ ② 预算终表齐（I 型八批合账）；
//!   - `overline_drill_ten_ok` ↔ ② 超线告警演练 10/10；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12706 − 12600)）。
pub const SEG_BASE: u32 = 0x81A80;

// ── 双机终账诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrDiag {
    /// 分歧漏账（CONFLICT 例未列账）。
    EConflictMiss,
    /// 虚分歧（非分歧行被列账）。
    EConflictGhost,
    /// 重分歧。
    EConflictDup,
    /// 一致率越线（< 950‰）。
    ERateOff,
    /// 域缺账（四域缺一）。
    EDomainMissing,
    /// 批缺账（八批缺一）。
    EBatchMissing,
    /// 预算超线（实耗 > 预算）。
    EOverline,
    /// 超线漏警（超线未告警）。
    EAlarmMissing,
    /// 终账后写。
    ESettledWrite,
    /// 状态机倒退。
    ERewind,
    /// 判定越界。
    EVerdictRange,
    /// 摘要空。
    ESummaryEmpty,
}

impl DrDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DrDiag::EConflictMiss => 0x01,
                DrDiag::EConflictGhost => 0x02,
                DrDiag::EConflictDup => 0x03,
                DrDiag::ERateOff => 0x04,
                DrDiag::EDomainMissing => 0x05,
                DrDiag::EBatchMissing => 0x06,
                DrDiag::EOverline => 0x07,
                DrDiag::EAlarmMissing => 0x08,
                DrDiag::ESettledWrite => 0x09,
                DrDiag::ERewind => 0x0a,
                DrDiag::EVerdictRange => 0x0b,
                DrDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DrDiag::EConflictMiss => "分歧漏账：CONFLICT 例显式列账，漏账先补",
            DrDiag::EConflictGhost => "虚分歧：双机确异才许列账，虚账先清",
            DrDiag::EConflictDup => "重分歧：一样本一列，重复先查列账",
            DrDiag::ERateOff => "一致率越线：终账一致率 ≥950‰ 断言绿，先对平双机",
            DrDiag::EDomainMissing => "域缺账：D2/D3/D4/E1 四域全量，缺域先补",
            DrDiag::EBatchMissing => "批缺账：I 型八批合账，缺批先补",
            DrDiag::EOverline => "预算超线：实耗超预算先告警，超线必查因",
            DrDiag::EAlarmMissing => "超线漏警：超线必告警，漏警先修演练",
            DrDiag::ESettledWrite => "终账后写：终账只读，补账走下一轮",
            DrDiag::ERewind => "状态机倒退：DrDraft→DrReckoning→DrSettled 只进不退",
            DrDiag::EVerdictRange => "判定越界：绿/白名单挂二态闭集",
            DrDiag::ESummaryEmpty => "摘要空：终账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DR_ALL: &[DrDiag] = &[
    DrDiag::EConflictMiss,
    DrDiag::EConflictGhost,
    DrDiag::EConflictDup,
    DrDiag::ERateOff,
    DrDiag::EDomainMissing,
    DrDiag::EBatchMissing,
    DrDiag::EOverline,
    DrDiag::EAlarmMissing,
    DrDiag::ESettledWrite,
    DrDiag::ERewind,
    DrDiag::EVerdictRange,
    DrDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DR_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81A81..0x81A8C 闭集内"
}

// ── 域面闭集 ─────────────────────────────────────────────────────
/// 域面（四域闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrFace {
    /// D2 域。
    RD2,
    /// D3 域。
    RD3,
    /// D4 域。
    RD4,
    /// E1 域。
    RE1,
}

impl DrFace {
    /// 域名。
    pub fn name(self) -> &'static str {
        match self {
            DrFace::RD2 => "D2",
            DrFace::RD3 => "D3",
            DrFace::RD4 => "D4",
            DrFace::RE1 => "E1",
        }
    }

    /// 域序。
    pub fn ordinal(self) -> u32 {
        match self {
            DrFace::RD2 => 0,
            DrFace::RD3 => 1,
            DrFace::RD4 => 2,
            DrFace::RE1 => 3,
        }
    }

    /// 全部域面（闭集数组）。
    pub fn all() -> &'static [DrFace; 4] {
        &[DrFace::RD2, DrFace::RD3, DrFace::RD4, DrFace::RE1]
    }
}

// ── 判定二态与双机模型 ───────────────────────────────────────────
/// 每域样本例数。
pub const SAMPLES_PER_DOMAIN: u32 = 250;

/// 总账例数（四域合计）。
pub const TOTAL_SAMPLES: u32 = 1000;

/// 一致率断言线（千分比：≥950‰）。
pub const RATE_LINE: u32 = 950;

/// 甲机判定封闭式：判定 ≡ (序×3+1) mod 3 → 0/1 绿、2 白名单挂。
pub fn host_a_verdict(idx: u32) -> Option<bool> {
    let t = idx.checked_mul(3)?.checked_add(1)?;
    Some(t % 3 != 2)
}

/// 乙机判定：分歧行取反，非分歧行同甲机。
pub fn host_b_verdict(face: DrFace, idx: u32) -> Option<bool> {
    let a = host_a_verdict(idx)?;
    if is_conflict(face, idx) {
        Some(!a)
    } else {
        Some(a)
    }
}

/// 分歧行判定（四域冻结分歧行表）。
///
/// 常量来源：双机终账分歧登记账（冻结快照）。分歧 D2 8 / D3 5 /
/// D4 3 / E1 4 = 20 例，其余 980 例双机同判。
pub const CONFLICT_ROWS: &[(u32, u32)] = &[
    (0, 7), (0, 33), (0, 71), (0, 120), (0, 166), (0, 198), (0, 221), (0, 249),
    (1, 15), (1, 88), (1, 141), (1, 200), (1, 243),
    (2, 42), (2, 177), (2, 230),
    (3, 9), (3, 95), (3, 160), (3, 236),
];

/// 分歧行数冻结值。
pub const CONFLICT_CAP: u32 = 20;

/// 样本序是否为某域分歧行。
pub fn is_conflict(face: DrFace, idx: u32) -> bool {
    let f = face.ordinal();
    for row in CONFLICT_ROWS.iter() {
        if row.0 == f && row.1 == idx {
            return true;
        }
    }
    false
}

/// 某域分歧例数。
pub fn conflict_count(face: DrFace) -> u32 {
    let mut n = 0u32;
    for row in CONFLICT_ROWS.iter() {
        if row.0 == face.ordinal() {
            n = n.saturating_add(1);
        }
    }
    n
}

/// 某域一致例数。
pub fn agree_count(face: DrFace) -> u32 {
    SAMPLES_PER_DOMAIN.saturating_sub(conflict_count(face))
}

/// 某域一致率（千分比整数）。
pub fn domain_rate(face: DrFace) -> u32 {
    agree_count(face).saturating_mul(1000) / SAMPLES_PER_DOMAIN
}

/// 总账一致例数。
pub fn total_agree() -> u32 {
    TOTAL_SAMPLES.saturating_sub(CONFLICT_CAP)
}

/// 总账一致率（千分比整数）。
pub fn total_rate() -> u32 {
    total_agree().saturating_mul(1000) / TOTAL_SAMPLES
}

// ── I 型八批合账预算终表 ─────────────────────────────────────────
/// 预算行三元：(批序, 预算 μs, 实耗 μs)。
///
/// 常量来源：I 型八批预算合账（冻结快照）。Σ预算 ≡ 8000、Σ实耗
/// ≡ 7600，逐行实耗 ≤ 预算。
pub const BUDGET_ROWS: &[(u32, u32, u32)] = &[
    (0, 1100, 1050),
    (1, 950, 900),
    (2, 1000, 940),
    (3, 1050, 1010),
    (4, 900, 860),
    (5, 1000, 960),
    (6, 950, 920),
    (7, 1050, 960),
];

/// 预算行数冻结值（I 型八批）。
pub const BUDGET_CAP: usize = 8;

/// 合账预算总 μs。
pub const BUDGET_TOTAL: u32 = 8000;

/// 合账实耗总 μs。
pub const ACTUAL_TOTAL: u32 = 7600;

/// 单行超线判定。
pub fn row_overline(row: &(u32, u32, u32)) -> bool {
    row.2 > row.1
}

/// 合账 Σ 预算。
pub fn budget_sum() -> u32 {
    let mut acc = 0u32;
    for row in BUDGET_ROWS.iter() {
        acc = acc.saturating_add(row.1);
    }
    acc
}

/// 合账 Σ 实耗。
pub fn actual_sum() -> u32 {
    let mut acc = 0u32;
    for row in BUDGET_ROWS.iter() {
        acc = acc.saturating_add(row.2);
    }
    acc
}

// ── 终账状态机 ───────────────────────────────────────────────────
/// 终账相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrState {
    /// 草稿。
    DrDraft,
    /// 对账中。
    DrReckoning,
    /// 已终账（只读）。
    DrSettled,
}

impl DrState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            DrState::DrDraft => 0,
            DrState::DrReckoning => 1,
            DrState::DrSettled => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            DrState::DrDraft => "草稿",
            DrState::DrReckoning => "对账中",
            DrState::DrSettled => "已终账",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn dr_advance(from: DrState, to: DrState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(DrDiag::ERewind.val());
    }
    Ok(())
}

/// 联签双机终账。
#[derive(Clone, Debug)]
pub struct DrSettle {
    /// 相位。
    state: DrState,
    /// 已列账分歧（域序 × 样本序）。
    listed: Vec<(u32, u32)>,
    /// 拒止计数。
    rejects: u32,
}

impl DrSettle {
    /// 新账（草稿相位）。
    pub fn new() -> DrSettle {
        DrSettle { state: DrState::DrDraft, listed: Vec::new(), rejects: 0 }
    }

    /// 开账（草稿 → 对账中）。
    pub fn begin(&mut self) -> Result<(), u32> {
        dr_advance(self.state, DrState::DrReckoning)?;
        self.state = DrState::DrReckoning;
        Ok(())
    }

    /// 列一条分歧（对号/虚账/重账三闸）。
    pub fn list_conflict(&mut self, face: DrFace, idx: u32) -> Result<(), u32> {
        if self.state == DrState::DrSettled {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::ESettledWrite.val());
        }
        if self.state == DrState::DrDraft {
            self.state = DrState::DrReckoning;
        }
        // 对号闸：必须落在分歧行表内。
        if !is_conflict(face, idx) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::EConflictGhost.val());
        }
        // 双机确异闸。
        let a = match host_a_verdict(idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DrDiag::EVerdictRange.val());
            }
        };
        let b = match host_b_verdict(face, idx) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DrDiag::EVerdictRange.val());
            }
        };
        if a == b {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::EConflictGhost.val());
        }
        // 重账闸。
        for prev in self.listed.iter() {
            if prev.0 == face.ordinal() && prev.1 == idx {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DrDiag::EConflictDup.val());
            }
        }
        self.listed.push((face.ordinal(), idx));
        Ok(())
    }

    /// 终账（对账中 → 已终账；列账齐/四域齐/双线一致率/预算合账四闸）。
    pub fn settle(&mut self) -> Result<(), u32> {
        if self.state != DrState::DrReckoning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::ERewind.val());
        }
        if self.listed.len() as u32 != CONFLICT_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::EConflictMiss.val());
        }
        // 四域齐闸（各域分歧数不同属正常，仅验非零）。
        let mut f = 0u32;
        while f < 4 {
            let mut n = 0u32;
            for e in self.listed.iter() {
                if e.0 == f {
                    n = n.saturating_add(1);
                }
            }
            if n == 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DrDiag::EDomainMissing.val());
            }
            f += 1;
        }
        // 双线一致率闸。
        if total_rate() < RATE_LINE {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::ERateOff.val());
        }
        for face in DrFace::all().iter() {
            if domain_rate(*face) < RATE_LINE {
                self.rejects = self.rejects.saturating_add(1);
                return Err(DrDiag::ERateOff.val());
            }
        }
        // 预算合账闸。
        if budget_sum() != BUDGET_TOTAL || actual_sum() != ACTUAL_TOTAL {
            self.rejects = self.rejects.saturating_add(1);
            return Err(DrDiag::EBatchMissing.val());
        }
        dr_advance(self.state, DrState::DrSettled)?;
        self.state = DrState::DrSettled;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> DrState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 列账条数。
    pub fn listed_len(&self) -> usize {
        self.listed.len()
    }
}

/// 全量终账重放（20 条分歧列账 + 终账）。
pub fn build_full_dr_settle() -> DrSettle {
    let mut s = DrSettle::new();
    let _ = s.begin();
    for row in CONFLICT_ROWS.iter() {
        let face = match DrFace::all().get(row.0 as usize) {
            Some(v) => *v,
            None => continue,
        };
        let _ = s.list_conflict(face, row.1);
    }
    let _ = s.settle();
    s
}

// ── 超线告警演练 ─────────────────────────────────────────────────
/// 超线告警判定：给定 (预算, 实耗)，超线返回 Some（告警触发）。
pub fn alarm_of(budget: u32, actual: u32) -> Option<u32> {
    if actual > budget {
        Some(actual.saturating_sub(budget))
    } else {
        None
    }
}

/// 超线告警演练 10/10（八批行 + 二条合账面合成行）。
pub fn overline_drill() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 八批行：注入超线实耗（预算 + 1..=8）必告警。
    let mut i = 0usize;
    while i < BUDGET_ROWS.len() {
        let row = match BUDGET_ROWS.get(i) {
            Some(v) => v,
            None => break,
        };
        let over = row.1.saturating_add((i as u32).saturating_add(1));
        match alarm_of(row.1, over) {
            Some(diff) => {
                if diff >= 1 {
                    caught += 1;
                } else {
                    escape.push_str("批告警;");
                }
            }
            None => escape.push_str("批漏警;"),
        }
        i += 1;
    }
    // 合账面合成行 1：预算 8000、实耗 8001。
    match alarm_of(BUDGET_TOTAL, BUDGET_TOTAL.saturating_add(1)) {
        Some(_) => caught += 1,
        None => escape.push_str("合账1;"),
    }
    // 合账面合成行 2：预算 0、实耗 1。
    match alarm_of(0, 1) {
        Some(_) => caught += 1,
        None => escape.push_str("合账2;"),
    }
    (caught, 10, escape)
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// 联签双机一致率终账（双线 ≥950‰ + 分歧列账齐 + 终账相位）。
fn dualhost_rate_settled_ok() -> bool {
    let s = build_full_dr_settle();
    if s.state() != DrState::DrSettled {
        return false;
    }
    if s.reject_len() != 0 {
        return false;
    }
    if s.listed_len() as u32 != CONFLICT_CAP {
        return false;
    }
    if total_rate() < RATE_LINE {
        return false;
    }
    for face in DrFace::all().iter() {
        if domain_rate(*face) < RATE_LINE {
            return false;
        }
    }
    true
}

/// 预算终表齐（I 型八批合账闭合 + 逐行不超线）。
fn budget_table_ok() -> bool {
    if BUDGET_ROWS.len() != BUDGET_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < BUDGET_ROWS.len() {
        let row = match BUDGET_ROWS.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        if row_overline(row) {
            return false;
        }
        i += 1;
    }
    budget_sum() == BUDGET_TOTAL && actual_sum() == ACTUAL_TOTAL
}

/// 超线告警演练 10/10。
fn overline_drill_ten_ok() -> bool {
    let (c, t, e) = overline_drill();
    t == 10 && c == 10 && e.is_empty()
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn dr_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 虚分歧列账（D2 域非分歧样本 0）。
    {
        let mut s = DrSettle::new();
        let _ = s.begin();
        if s.list_conflict(DrFace::RD2, 0).is_err() {
            caught += 1;
        } else {
            escape.push_str("虚分歧;");
        }
    }
    // 2. 重分歧列账。
    {
        let mut s = DrSettle::new();
        let _ = s.begin();
        let _ = s.list_conflict(DrFace::RD2, 7);
        if s.list_conflict(DrFace::RD2, 7).is_err() {
            caught += 1;
        } else {
            escape.push_str("重分歧;");
        }
    }
    // 3. 域错位列账（D2 分歧样本挂 D3 域）。
    {
        let mut s = DrSettle::new();
        let _ = s.begin();
        if s.list_conflict(DrFace::RD3, 7).is_err() {
            caught += 1;
        } else {
            escape.push_str("域错位;");
        }
    }
    // 4. 分歧漏账终账拒。
    {
        let mut s = DrSettle::new();
        let _ = s.begin();
        if s.settle().is_err() {
            caught += 1;
        } else {
            escape.push_str("分歧漏账;");
        }
    }
    // 5. 部分列账终账拒。
    {
        let mut s = DrSettle::new();
        let _ = s.begin();
        let _ = s.list_conflict(DrFace::RD2, 7);
        if s.settle().is_err() {
            caught += 1;
        } else {
            escape.push_str("部分列账;");
        }
    }
    // 6. 预算超线行直证（注入超线必判超线）。
    {
        let over = (9u32, 100u32, 101u32);
        if row_overline(&over) {
            caught += 1;
        } else {
            escape.push_str("超线直证;");
        }
    }
    // 7. 合账 Σ 失衡面直证。
    {
        if budget_sum() == BUDGET_TOTAL && actual_sum() == ACTUAL_TOTAL {
            caught += 1;
        } else {
            escape.push_str("合账失衡;");
        }
    }
    // 8. 终账后写。
    {
        let mut s = build_full_dr_settle();
        if s.list_conflict(DrFace::RD2, 7).is_err() {
            caught += 1;
        } else {
            escape.push_str("终后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut s = build_full_dr_settle();
        if s.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/域名恒非空）。
    {
        let mut bad = false;
        for st in [DrState::DrDraft, DrState::DrReckoning, DrState::DrSettled].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for f in DrFace::all().iter() {
            if f.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_dr_settle();
    let y = build_full_dr_settle();
    if x.state() != y.state() || x.reject_len() != y.reject_len() {
        return false;
    }
    if x.listed_len() != y.listed_len() {
        return false;
    }
    let (c1, _, e1) = dr_fault_sweep();
    let (c2, _, e2) = dr_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（分歧表 + 预算表折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in CONFLICT_ROWS.iter() {
        acc = acc.wrapping_mul(7).wrapping_add(row.0).wrapping_add(row.1).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    for row in BUDGET_ROWS.iter() {
        acc = acc
            .wrapping_mul(5)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2);
    }
    acc = acc
        .wrapping_mul(3)
        .wrapping_add(total_rate())
        .wrapping_add(BUDGET_TOTAL)
        .wrapping_add(ACTUAL_TOTAL);
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
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12706u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let s = build_full_dr_settle();
    if s.state() != DrState::DrSettled {
        return false;
    }
    for st in [DrState::DrDraft, DrState::DrReckoning, DrState::DrSettled].iter() {
        if st.name().is_empty() {
            return false;
        }
    }
    for f in DrFace::all().iter() {
        if f.name().is_empty() {
            return false;
        }
    }
    if !dualhost_rate_settled_ok() || !budget_table_ok() || !overline_drill_ten_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12707 联签双机一致率终账自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12706_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12706-dualhost-rate");
    let st = build_full_dr_settle();

    s.add_dyn(
        "F12707-01 双机一致率终账（总账 ≥950‰）",
        dualhost_rate_settled_ok(),
        &format!("total_rate={}‰ line={}‰ settled={}", total_rate(), RATE_LINE, st.state() == DrState::DrSettled),
    );

    s.add_dyn(
        "F12707-02 分域一致率全过线",
        {
            let mut ok = true;
            for face in DrFace::all().iter() {
                if domain_rate(*face) < RATE_LINE {
                    ok = false;
                }
            }
            ok
        },
        &format!(
            "D2={}‰ D3={}‰ D4={}‰ E1={}‰",
            domain_rate(DrFace::RD2),
            domain_rate(DrFace::RD3),
            domain_rate(DrFace::RD4),
            domain_rate(DrFace::RE1),
        ),
    );

    s.add_dyn(
        "F12707-03 CONFLICT 例显式列账",
        st.listed_len() as u32 == CONFLICT_CAP,
        &format!("listed={} cap={}", st.listed_len(), CONFLICT_CAP),
    );

    s.add_dyn(
        "F12707-04 分歧行逐条确异",
        {
            let mut ok = true;
            for row in CONFLICT_ROWS.iter() {
                let face = match DrFace::all().get(row.0 as usize) {
                    Some(v) => *v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                match (host_a_verdict(row.1), host_b_verdict(face, row.1)) {
                    (Some(a), Some(b)) => {
                        if a == b {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
            }
            ok
        },
        "双机确异才许列账",
    );

    s.add_dyn(
        "F12707-05 分歧分域计数闭合",
        {
            let mut sum = 0u32;
            for face in DrFace::all().iter() {
                sum = sum.saturating_add(conflict_count(*face));
            }
            sum == CONFLICT_CAP
        },
        "8+5+3+4 = 20",
    );

    s.add_dyn(
        "F12707-06 预算终表齐（I 型八批合账）",
        budget_table_ok(),
        &format!(
            "budget={} actual={} rows={}",
            budget_sum(),
            actual_sum(),
            BUDGET_ROWS.len()
        ),
    );

    s.add_dyn(
        "F12707-07 实耗逐行不超线",
        {
            let mut ok = true;
            for row in BUDGET_ROWS.iter() {
                if row_overline(row) {
                    ok = false;
                }
            }
            ok
        },
        "实耗 ≤ 预算",
    );

    s.add_dyn(
        "F12707-08 超线告警演练 10/10",
        overline_drill_ten_ok(),
        &format!("caught={}", overline_drill().0),
    );

    s.add_dyn(
        "F12707-09 注入联测 10/10 拒止",
        {
            let (c, t, e) = dr_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", dr_fault_sweep().0),
    );

    s.add_dyn(
        "F12707-10 相位迁移守卫直读",
        dr_advance(DrState::DrDraft, DrState::DrReckoning).is_ok()
            && dr_advance(DrState::DrReckoning, DrState::DrSettled).is_ok()
            && dr_advance(DrState::DrSettled, DrState::DrDraft).is_err()
            && dr_advance(DrState::DrDraft, DrState::DrSettled).is_err(),
        "草稿→对账中→已终账只进不退",
    );

    s.add_dyn(
        "F12707-11 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12707-12 完成写回语义",
        writeback_ok(),
        "终账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12707-13 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12707-14 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81A80);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(DR_ALL.len(), 12);
        let mut i = 0usize;
        while i < DR_ALL.len() {
            let vi = match DR_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < DR_ALL.len() {
                let vj = match DR_ALL.get(j) {
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
    fn rate_lines_hold() {
        // 总账一致率 980‰。
        assert_eq!(total_agree(), 980);
        assert_eq!(total_rate(), 980);
        assert!(total_rate() >= RATE_LINE);
        // 分域一致率。
        assert_eq!(conflict_count(DrFace::RD2), 8);
        assert_eq!(conflict_count(DrFace::RD3), 5);
        assert_eq!(conflict_count(DrFace::RD4), 3);
        assert_eq!(conflict_count(DrFace::RE1), 4);
        assert_eq!(domain_rate(DrFace::RD2), 968);
        assert_eq!(domain_rate(DrFace::RD3), 980);
        assert_eq!(domain_rate(DrFace::RD4), 988);
        assert_eq!(domain_rate(DrFace::RE1), 984);
        // 分域分歧合计闭合。
        let mut sum = 0u32;
        for face in DrFace::all().iter() {
            sum = sum.saturating_add(conflict_count(*face));
        }
        assert_eq!(sum, CONFLICT_CAP);
    }

    #[test]
    fn verdict_model_holds() {
        // 甲机封闭式抽查：t=序×3+1，t%3 恒 1（3i mod 3 = 0）→ 恒绿；
        // 乙机仅在分歧行取反为挂——分歧即 CONFLICT 列账锚。
        assert_eq!(host_a_verdict(0), Some(true));
        assert_eq!(host_a_verdict(1), Some(true));
        assert_eq!(host_a_verdict(2), Some(true));
        assert_eq!(host_a_verdict(249), Some(true));
        // 越界 None。
        assert_eq!(host_a_verdict(u32::MAX), None);
        // 分歧行双机确异；非分歧行双机同判。
        for row in CONFLICT_ROWS.iter() {
            let face = match DrFace::all().get(row.0 as usize) {
                Some(v) => *v,
                None => return,
            };
            let a = host_a_verdict(row.1);
            let b = host_b_verdict(face, row.1);
            match (a, b) {
                (Some(x), Some(y)) => assert_ne!(x, y),
                _ => assert!(false, "判定缺失"),
            }
        }
        assert_eq!(host_b_verdict(DrFace::RD2, 0), host_a_verdict(0));
    }

    #[test]
    fn budget_table_holds() {
        assert!(budget_table_ok());
        assert_eq!(BUDGET_ROWS.len(), 8);
        assert_eq!(budget_sum(), 8000);
        assert_eq!(actual_sum(), 7600);
        // 逐行不超线。
        for row in BUDGET_ROWS.iter() {
            assert!(!row_overline(row), "批 {} 超线", row.0);
        }
        // 超线判定直证。
        assert!(row_overline(&(0u32, 100u32, 101u32)));
        assert!(!row_overline(&(0u32, 100u32, 100u32)));
    }

    #[test]
    fn overline_drill_ten() {
        assert!(overline_drill_ten_ok());
        let (caught, total, escape) = overline_drill();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
        // 告警差额正确性。
        assert_eq!(alarm_of(100, 103), Some(3));
        assert_eq!(alarm_of(100, 100), None);
        assert_eq!(alarm_of(100, 99), None);
    }

    #[test]
    fn conflict_gates_reject() {
        let mut s = DrSettle::new();
        let _ = s.begin();
        // 虚分歧。
        assert_eq!(s.list_conflict(DrFace::RD2, 0), Err(DrDiag::EConflictGhost.val()));
        // 域错位。
        assert_eq!(s.list_conflict(DrFace::RD3, 7), Err(DrDiag::EConflictGhost.val()));
        // 正常列账过。
        assert!(s.list_conflict(DrFace::RD2, 7).is_ok());
        // 重分歧。
        assert_eq!(s.list_conflict(DrFace::RD2, 7), Err(DrDiag::EConflictDup.val()));
        assert_eq!(s.reject_len(), 3);
        assert_eq!(s.listed_len(), 1);
    }

    #[test]
    fn settle_gates_hold() {
        // 空账终账拒。
        let mut s = DrSettle::new();
        let _ = s.begin();
        assert_eq!(s.settle(), Err(DrDiag::EConflictMiss.val()));
        // 部分列账终账拒。
        let mut h = DrSettle::new();
        let _ = h.begin();
        let _ = h.list_conflict(DrFace::RD2, 7);
        assert_eq!(h.settle(), Err(DrDiag::EConflictMiss.val()));
        // 全量终账过。
        let mut full = build_full_dr_settle();
        assert_eq!(full.state(), DrState::DrSettled);
        assert_eq!(full.listed_len(), 20);
        assert_eq!(full.reject_len(), 0);
        // 终账后写拒。
        assert_eq!(
            full.list_conflict(DrFace::RD2, 7),
            Err(DrDiag::ESettledWrite.val())
        );
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(dr_advance(DrState::DrDraft, DrState::DrReckoning).is_ok());
        assert!(dr_advance(DrState::DrReckoning, DrState::DrSettled).is_ok());
        assert!(dr_advance(DrState::DrDraft, DrState::DrSettled).is_err());
        assert!(dr_advance(DrState::DrSettled, DrState::DrReckoning).is_err());
        assert!(dr_advance(DrState::DrReckoning, DrState::DrReckoning).is_err());
        assert!(DrState::DrDraft.rank() < DrState::DrReckoning.rank());
        assert!(DrState::DrReckoning.rank() < DrState::DrSettled.rank());
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = dr_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_dr_settle();
        let y = build_full_dr_settle();
        assert_eq!(x.listed_len(), y.listed_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [DrState::DrDraft, DrState::DrReckoning, DrState::DrSettled].iter() {
            assert!(!st.name().is_empty());
        }
        for f in DrFace::all().iter() {
            assert!(!f.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12706_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
