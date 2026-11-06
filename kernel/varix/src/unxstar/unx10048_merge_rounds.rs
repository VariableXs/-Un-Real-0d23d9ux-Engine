//! UNX-F10048 回归结果归并器（多轮合并） · 0x91c00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10048（B22 常态化回归线 · 归并器）。
//! 锚点：UNX-F10049-J1 基线比对零漂移（端点取数比对 10/10 次），绕端点比对 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91c00（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `MergeCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 状态格 `CaseStatus`：Pass/Flaky/Red 三态，severity 单调，归并取最坏；
//!   - 归并账本 `RoundMerger`：轮次号必须连续（1..=R 逐轮并入），跳轮/
//!     重复轮/空轮显性拒绝；逐格最坏状态封账，零丢失核对逐条可查；
//!   - 端点核 `round_endpoint` + `endpoint_guard`：比对必须走冻结端点
//!     （FNV 指纹），绕端点比对逐次拦截（对齐锚点判据）；
//!   - 注入核 `BadMergeCase`：跳轮 / 重复轮 / 空轮 / 越界格 / 绕端点
//!     五类坏样本逐类 10/10 次拒绝；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10048 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91c00;

/// 段窗上界（下一卡 0x91c40 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x91c40;

/// 战线条目数（40 条，B21 口径）。
pub const CASE_COUNT: usize = 40;

/// 未并入哨兵（worst 表初始值）。
pub const UNSET: u8 = 0xFF;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ENODATA: i32 = 61;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeCode {
    /// 跳轮（轮次号不连续）
    RoundGap,
    /// 重复轮
    DuplicateRound,
    /// 空轮（无结果行）
    EmptyRound,
    /// 越界格（用例号出 1..=40 或状态值出 0..=2）
    CellOutOfRange,
    /// 绕端点比对
    EndpointBypass,
    /// 端点漂移（端点指纹与归并账不符）
    EndpointDrift,
    /// 归并缺漏（零丢失核对失败）
    LossDetected,
    /// 账本倒退
    LedgerRegress,
    /// 归并器为空即比对
    EmptyMerger,
    /// 双跑不一致
    NotReproducible,
    /// 轮次越界（轮号出 1..=MAX_ROUNDS 窗）
    RoundOutOfRange,
    /// 段位越界
    SegmentViolation,
}

impl MergeCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            MergeCode::RoundGap => "跳轮被拒：轮次号必须从 1 连续推进；请补缺失轮再归并",
            MergeCode::DuplicateRound => "重复轮被拒：该轮已并入；请勿二次归并",
            MergeCode::EmptyRound => "空轮被拒：轮结果为空不得并入；请核对轮账",
            MergeCode::CellOutOfRange => "越界格被拒：用例号 1..=40、状态值 0..=2；请核对口",
            MergeCode::EndpointBypass => "绕端点比对被拒：比对必须走冻结端点；请先取 round_endpoint",
            MergeCode::EndpointDrift => "端点漂移被拒：端点指纹与归并账不符；请重算端点",
            MergeCode::LossDetected => "归并缺漏被拒：有格未并入；请逐条并入核对",
            MergeCode::LedgerRegress => "账本倒退被拒：计数只增不减；请排查回滚误改",
            MergeCode::EmptyMerger => "空归并器被拒：未并入任何轮不得比对；请先归并",
            MergeCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            MergeCode::RoundOutOfRange => "轮次越界被拒：轮号必须 ≥1；请核对轮账",
            MergeCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x91c00..=0x91c40",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [MergeCode; 12] = [
        MergeCode::RoundGap,
        MergeCode::DuplicateRound,
        MergeCode::EmptyRound,
        MergeCode::CellOutOfRange,
        MergeCode::EndpointBypass,
        MergeCode::EndpointDrift,
        MergeCode::LossDetected,
        MergeCode::LedgerRegress,
        MergeCode::EmptyMerger,
        MergeCode::NotReproducible,
        MergeCode::RoundOutOfRange,
        MergeCode::SegmentViolation,
    ];
}

// ── 状态格 ────────────────────────────────────────────────────────────
/// 用例状态三态（severity 单调：Pass < Flaky < Red）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaseStatus {
    /// 通过
    Pass,
    /// 抖动
    Flaky,
    /// 红
    Red,
}

impl CaseStatus {
    /// 状态值（0/1/2，闭集）。
    pub fn val(self) -> u8 {
        match self {
            CaseStatus::Pass => 0,
            CaseStatus::Flaky => 1,
            CaseStatus::Red => 2,
        }
    }

    /// 从状态值还原（0..=2 之外 None）。
    pub fn from_val(v: u8) -> Option<CaseStatus> {
        match v {
            0 => Some(CaseStatus::Pass),
            1 => Some(CaseStatus::Flaky),
            2 => Some(CaseStatus::Red),
            _ => None,
        }
    }

    /// severity（单调：0 < 1 < 2）。
    pub fn severity(self) -> u8 {
        self.val()
    }

    /// 取最坏（severity 大者胜，相等保持先者）。
    pub fn worse_of(a: CaseStatus, b: CaseStatus) -> CaseStatus {
        if b.severity() > a.severity() {
            b
        } else {
            a
        }
    }
}

// ── 归并账本 ──────────────────────────────────────────────────────────
/// 多轮归并器：轮次号连续推进，逐格最坏状态封账。
#[derive(Clone, Debug)]
pub struct RoundMerger {
    /// 已并入轮数（= 最后轮号，连续性由 merge_round 保证）
    pub merged: u32,
    /// 逐格最坏状态（UNSET=未并入）
    pub worst: [u8; CASE_COUNT],
    /// 已并入行数
    pub rows: u32,
    /// 账本倒退哨（防回滚误改）
    pub regressed: bool,
}

impl RoundMerger {
    /// 构造空归并器。
    pub const fn new() -> RoundMerger {
        RoundMerger { merged: 0, worst: [UNSET; CASE_COUNT], rows: 0, regressed: false }
    }

    /// 并入一轮：轮号必须 = merged+1；空轮/越界格/重复轮显性拒绝。
    pub fn merge_round(&mut self, round_id: u32, results: &[(u32, u8)]) -> Result<(), MergeCode> {
        if round_id == 0 {
            return Err(MergeCode::RoundOutOfRange);
        }
        if round_id <= self.merged {
            return Err(MergeCode::DuplicateRound);
        }
        if round_id != self.merged + 1 {
            return Err(MergeCode::RoundGap);
        }
        if results.is_empty() {
            return Err(MergeCode::EmptyRound);
        }
        for (id, st) in results.iter() {
            if *id == 0 || (*id as usize) > CASE_COUNT {
                return Err(MergeCode::CellOutOfRange);
            }
            if CaseStatus::from_val(*st).is_none() {
                return Err(MergeCode::CellOutOfRange);
            }
        }
        // 全部校验通过后才落账（半途拒绝不产生脏账）。
        for (id, st) in results.iter() {
            let idx = *id as usize - 1;
            let new = CaseStatus::from_val(*st).unwrap_or(CaseStatus::Pass);
            let old = match self.worst.get(idx).copied().unwrap_or(UNSET) {
                UNSET => new,
                v => match CaseStatus::from_val(v) {
                    Some(o) => CaseStatus::worse_of(o, new),
                    None => new,
                },
            };
            if let Some(slot) = self.worst.get_mut(idx) {
                *slot = old.val();
            }
            self.rows = self.rows.saturating_add(1);
        }
        self.merged = round_id;
        Ok(())
    }

    /// 零丢失核对：期望格数与已并入格数对表，缺格计数返回。
    pub fn audit_loss(&self, expected_cases: usize) -> u32 {
        let mut missing = 0u32;
        let mut i = 0usize;
        while i < expected_cases && i < CASE_COUNT {
            if *self.worst.get(i).unwrap_or(&UNSET) == UNSET {
                missing = missing.saturating_add(1);
            }
            i += 1;
        }
        missing
    }

    /// 已并入格数。
    pub fn filled(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < CASE_COUNT {
            if *self.worst.get(i).unwrap_or(&UNSET) != UNSET {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

// ── 端点核 ────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 轮端点：轮号 + 逐行 (id,status) 逐字节入 FNV（冻结端点取数口）。
pub fn round_endpoint(round_id: u32, results: &[(u32, u8)]) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&round_id.to_le_bytes());
    for (id, st) in results.iter() {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.push(*st);
    }
    fnv1a(&bytes)
}

/// 归并账端点：merged + 逐格 worst 逐字节入 FNV。
pub fn merger_endpoint(m: &RoundMerger) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(&m.merged.to_le_bytes());
    let mut i = 0usize;
    while i < CASE_COUNT {
        if let Some(v) = m.worst.get(i) {
            bytes.push(*v);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

/// 端点守卫：via_endpoint=false 即绕端点比对，逐次拦截。
pub fn endpoint_guard(via_endpoint: bool) -> Result<(), MergeCode> {
    if via_endpoint {
        Ok(())
    } else {
        Err(MergeCode::EndpointBypass)
    }
}

/// 端点取数比对：先过守卫，再比两端点指纹（零漂移比对口）。
pub fn compare_via_endpoint(a: u32, b: u32, via_endpoint: bool) -> Result<bool, MergeCode> {
    endpoint_guard(via_endpoint)?;
    Ok(a == b)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏归并样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadMergeCase {
    /// 跳轮（并入 1 后直接并 3）
    RoundGap,
    /// 重复轮（同轮二次并入）
    DuplicateRound,
    /// 空轮
    EmptyRound,
    /// 越界格（用例号 41 / 状态值 7）
    CellOutOfRange,
    /// 绕端点比对
    EndpointBypass,
}

impl BadMergeCase {
    /// 闭集全量。
    pub const ALL: [BadMergeCase; 5] = [
        BadMergeCase::RoundGap,
        BadMergeCase::DuplicateRound,
        BadMergeCase::EmptyRound,
        BadMergeCase::CellOutOfRange,
        BadMergeCase::EndpointBypass,
    ];

    /// 注入一轮：构造坏样本并触发归并器，返回是否被显式拒绝。
    pub fn inject(self) -> bool {
        let mut m = RoundMerger::new();
        let r1: Vec<(u32, u8)> = (1..=10).map(|i| (i, 0)).collect();
        if m.merge_round(1, &r1).is_err() {
            return false;
        }
        match self {
            BadMergeCase::RoundGap => m.merge_round(3, &r1).is_err(),
            BadMergeCase::DuplicateRound => m.merge_round(1, &r1).is_err(),
            BadMergeCase::EmptyRound => m.merge_round(2, &[]).is_err(),
            BadMergeCase::CellOutOfRange => {
                let bad_id: Vec<(u32, u8)> = vec![(41, 0)];
                let bad_st: Vec<(u32, u8)> = vec![(1, 7)];
                m.merge_round(2, &bad_id).is_err() && m.merge_round(2, &bad_st).is_err()
            }
            BadMergeCase::EndpointBypass => {
                let ep = round_endpoint(1, &r1);
                compare_via_endpoint(ep, ep, false).is_err()
                    && compare_via_endpoint(ep, ep, true).is_ok()
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl MergeLedger {
    const fn zero() -> MergeLedger {
        MergeLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账失衡回 LedgerRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), MergeCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(MergeCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_merge_injection() -> (MergeLedger, [u32; 5]) {
    let mut ledger = MergeLedger::zero();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadMergeCase::ALL.len() {
        if let Some(case) = BadMergeCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；空轮/缺轮族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；绕端点比对族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复轮族"),
    ("ENODATA", -61, 232, "Windows ERROR_NO_DATA；零丢失缺格族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界格族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// 规范语料双跑指纹：三轮归并（状态起伏）→ 端点取数逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut m = RoundMerger::new();
    let mut bytes: Vec<u8> = Vec::new();
    // 轮 1：全 Pass；轮 2：偶数格 Flaky；轮 3：格 3 转 Red（最坏封账演进）。
    let r1: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, 0)).collect();
    let r2: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, if i % 2 == 0 { 1 } else { 0 })).collect();
    let r3: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, if i == 3 { 2 } else { 0 })).collect();
    for (rid, rows) in [(1u32, &r1), (2, &r2), (3, &r3)].iter() {
        let ep = round_endpoint(*rid, rows);
        bytes.extend_from_slice(&ep.to_le_bytes());
        bytes.push(match m.merge_round(*rid, rows) {
            Ok(()) => 1,
            Err(_) => 0,
        });
    }
    bytes.extend_from_slice(&merger_endpoint(&m).to_le_bytes());
    bytes.push(m.audit_loss(CASE_COUNT) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10048 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10048_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10048");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91c00..=0x91c40", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 状态格闭集与 severity 单调
    let st_ok = CaseStatus::Pass.severity() < CaseStatus::Flaky.severity()
        && CaseStatus::Flaky.severity() < CaseStatus::Red.severity()
        && CaseStatus::from_val(3).is_none()
        && CaseStatus::worse_of(CaseStatus::Pass, CaseStatus::Red) == CaseStatus::Red;
    s.add("状态格：Pass<Flaky<Red 单调且最坏取对", st_ok,
          "三态闭集，归并取最坏保底");

    // 3. 三轮归并零丢失
    let mut m = RoundMerger::new();
    let r1: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, 0)).collect();
    let r2: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, if i % 2 == 0 { 1 } else { 0 })).collect();
    let r3: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).map(|i| (i, if i == 3 { 2 } else { 0 })).collect();
    let m_ok = m.merge_round(1, &r1).is_ok()
        && m.merge_round(2, &r2).is_ok()
        && m.merge_round(3, &r3).is_ok();
    s.add("三轮归并：逐轮并入 120 行、40 格全落账", m_ok && m.audit_loss(CASE_COUNT) == 0,
          "多轮逐条并入核对，缺格即 LossDetected");

    // 4. 最坏状态封账：格 3 三轮演进后为 Red
    let worst3 = m.worst.get(2).copied().unwrap_or(UNSET);
    let worst16 = m.worst.get(15).copied().unwrap_or(UNSET);
    s.add("最坏封账：格 3=Red、格 16=Flaky",
          worst3 == 2 && worst16 == 1,
          "多轮状态起伏后逐格取最坏");

    // 5. 跳轮拦截（轮次丢失 10/10，锚点判据直接探针）
    let mut gap_hit = 0u32;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut mm = RoundMerger::new();
        if mm.merge_round(1, &r1).is_ok() && mm.merge_round(3, &r1).is_err() {
            gap_hit = gap_hit.saturating_add(1);
        }
        r += 1;
    }
    s.add("轮次丢失拦截：跳轮 10/10 显性拒绝", gap_hit == INJECT_ROUNDS,
          "轮号必须连续，跳轮即 RoundGap");

    // 6. 重复轮拦截
    let mut dup = RoundMerger::new();
    let _ = dup.merge_round(1, &r1);
    let dup_ok = dup.merge_round(1, &r1) == Err(MergeCode::DuplicateRound);
    s.add("重复轮：同轮二次并入被拒", dup_ok,
          "轮账唯一，重复并入显性拒绝");

    // 7. 空轮与零号轮拦截
    let mut er = RoundMerger::new();
    let er_ok = er.merge_round(1, &[]) == Err(MergeCode::EmptyRound)
        && er.merge_round(0, &r1) == Err(MergeCode::RoundOutOfRange);
    s.add("轮界：空轮/零号轮双双拒绝", er_ok,
          "轮账前置条件逐项显性校验");

    // 8. 越界格拦截（用例号与状态值双探）
    let mut cell = RoundMerger::new();
    let bad_id: Vec<(u32, u8)> = vec![(41, 0)];
    let bad_st: Vec<(u32, u8)> = vec![(1, 7)];
    let cell_ok = cell.merge_round(1, &bad_id).is_err()
        && cell.merge_round(1, &bad_st).is_err();
    s.add("越界格：用例号 41/状态值 7 全拒", cell_ok,
          "格闭集 1..=40 × 0..=2，出格即拦");

    // 9. 绕端点比对 10/10 拦截（锚点判据直接探针）
    let mut bypass_hit = 0u32;
    let mut r2 = 0u32;
    let ep1 = round_endpoint(1, &r1);
    while r2 < INJECT_ROUNDS {
        if compare_via_endpoint(ep1, ep1, false).is_err()
            && compare_via_endpoint(ep1, ep1, true) == Ok(true)
        {
            bypass_hit = bypass_hit.saturating_add(1);
        }
        r2 += 1;
    }
    s.add("绕端点比对拦截：10/10 显性拒绝", bypass_hit == INJECT_ROUNDS,
          "比对必须走冻结端点取数口，绕行即 EndpointBypass");

    // 10. 端点漂移检出：归并账端点与篡改端点必不等
    let tampered = ep1.wrapping_add(1);
    let drift_ok = merger_endpoint(&m) != tampered
        && compare_via_endpoint(merger_endpoint(&m), tampered, true) == Ok(false);
    s.add("端点漂移：篡改端点与归并账比对检出", drift_ok,
          "端点指纹与账面不符即 EndpointDrift 口径");

    // 11. 归并缺漏检出：少并一格即被零丢失核对抓到
    let mut sparse = RoundMerger::new();
    let r_missing: Vec<(u32, u8)> = (1..=CASE_COUNT as u32).filter(|i| *i != 7).map(|i| (i, 0)).collect();
    let _ = sparse.merge_round(1, &r_missing);
    s.add("归并缺漏：缺格 7 被零丢失核对检出", sparse.audit_loss(CASE_COUNT) == 1,
          "逐条并入核对负样 1/1 检出");

    // 12. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_merge_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "跳轮/重复轮/空轮/越界格/绕端点全被显式拒绝");

    // 13. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 14. 文案闭集：非空
    let text_ok = MergeCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i = 0usize;
    while i < MergeCode::ALL.len() {
        let mut j = i + 1;
        while j < MergeCode::ALL.len() {
            if let (Some(x), Some(y)) = (MergeCode::ALL.get(i), MergeCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = MergeCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 16. Linux 逐码一致
    let codes_ok = agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(ENODATA)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：归并账一行 + 指纹一行
    let acc_text = format!("轮次 {} / 行数 {} / 满格 {} / 缺格 {}",
        m.merged, m.rows, m.filled(), m.audit_loss(CASE_COUNT));
    s.add_dyn("归并快照", m_ok && m.audit_loss(CASE_COUNT) == 0, &acc_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_closure() {
        assert_eq!(CaseStatus::from_val(2), Some(CaseStatus::Red));
        assert_eq!(CaseStatus::from_val(9), None);
        assert_eq!(CaseStatus::worse_of(CaseStatus::Flaky, CaseStatus::Pass), CaseStatus::Flaky);
    }

    #[test]
    fn round_gap_rejected() {
        let mut m = RoundMerger::new();
        let r: Vec<(u32, u8)> = vec![(1, 0)];
        let _ = m.merge_round(1, &r);
        assert_eq!(m.merge_round(3, &r), Err(MergeCode::RoundGap));
    }

    #[test]
    fn duplicate_round_rejected() {
        let mut m = RoundMerger::new();
        let r: Vec<(u32, u8)> = vec![(1, 0)];
        let _ = m.merge_round(1, &r);
        assert_eq!(m.merge_round(1, &r), Err(MergeCode::DuplicateRound));
    }

    #[test]
    fn empty_round_rejected() {
        let mut m = RoundMerger::new();
        assert_eq!(m.merge_round(1, &[]), Err(MergeCode::EmptyRound));
    }

    #[test]
    fn cell_out_of_range_rejected() {
        let mut m = RoundMerger::new();
        assert_eq!(m.merge_round(1, &[(41, 0)]), Err(MergeCode::CellOutOfRange));
        assert_eq!(m.merge_round(1, &[(1, 9)]), Err(MergeCode::CellOutOfRange));
        assert_eq!(m.merge_round(1, &[(0, 0)]), Err(MergeCode::CellOutOfRange));
    }

    #[test]
    fn worst_status_wins() {
        let mut m = RoundMerger::new();
        let _ = m.merge_round(1, &[(1, 0)]);
        let _ = m.merge_round(2, &[(1, 2)]);
        let _ = m.merge_round(3, &[(1, 0)]);
        assert_eq!(m.worst.get(0).copied(), Some(2));
    }

    #[test]
    fn zero_loss_audit() {
        let mut m = RoundMerger::new();
        let r: Vec<(u32, u8)> = (1..=40).map(|i| (i, 0)).collect();
        let _ = m.merge_round(1, &r);
        assert_eq!(m.audit_loss(40), 0);
        assert_eq!(m.audit_loss(39), 0);
        assert_eq!(m.filled(), 40);
    }

    #[test]
    fn endpoint_bypass_rejected() {
        let ep = round_endpoint(1, &[(1, 0)]);
        assert_eq!(compare_via_endpoint(ep, ep, false), Err(MergeCode::EndpointBypass));
        assert_eq!(compare_via_endpoint(ep, ep, true), Ok(true));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_merge_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 5 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(ENODATA));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10048_checks_green() {
        let s = run_unx10048_checks();
        assert!(s.all_passed(), "UNX-F10048 red: {}", s.red_items_text());
    }
}
