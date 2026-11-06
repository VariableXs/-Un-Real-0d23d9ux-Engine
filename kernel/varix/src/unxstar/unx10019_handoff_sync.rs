//! UNX-F10019 报表与 handoff 同步段 · 0x914C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10019（B21 波次报表线）。
//! 锚点：UNX-F10020-J1 收口小结落账（20 条判据达成+预算 6,540 核销零偏离），核销不符 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x914C0（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `SyncCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 同步注册表 `SyncRegistry`：报表版本 ↔ handoff 版本成对登记，
//!     逐对核齐（漂移 = 单侧登记或版本不等）；
//!   - 收口小结账 `SummaryLedger`：20 条判据逐条落账，缺条即虚报，
//!     预算 6,540 核销守恒（核销额累计 = 预算额）；
//!   - 状态机 `SyncState`：Collecting → Aligned → Handoff 单向推进，
//!     Handoff 后不可再改；
//!   - 注入核 `BadSyncCase`：单侧登记 / 版本漂移 / 判据缺条 / 预算超销 /
//!     重复核销五类坏样本逐类 10/10 次检出；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10019 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x914C0;

/// 收口判据条数（B21 波次线 20 条）。
pub const CRITERIA_N: usize = 20;

/// B21 预算（行数口径，卡面冻结值 6,540）。
pub const BUDGET: u32 = 6_540;

/// 同步对容量。
pub const SYNC_CAP: usize = 16;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EOVERFLOW: i32 = 75;
pub const EEXIST: i32 = 17;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 单侧登记（只报报表或只报 handoff）
    OneSidedEntry,
    /// 版本漂移（成对但版本不等）
    VersionDrift,
    /// 判据缺条（<20 条）
    MissingCriteria,
    /// 预算超销（核销>6,540）
    BudgetOverspend,
    /// 重复核销（同判据条双销）
    DuplicateWriteOff,
    /// 账本倒退
    LedgerRegress,
    /// 空版本登记
    EmptyVersion,
    /// 容量满
    SyncFull,
    /// Handoff 后改写
    HandoffRewrite,
    /// 未对齐即交接
    HandoffBeforeAlign,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl SyncCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            SyncCode::OneSidedEntry => "单侧登记被拒：报表版本与 handoff 版本必须\
                成对登记；请补齐另一侧",
            SyncCode::VersionDrift => "版本漂移检出：成对版本号不相等；请对齐\
                报表↔handoff 版本（判据原文：漂移 10/10 次检出）",
            SyncCode::MissingCriteria => "判据缺条被拒：收口小结必须 20 条齐备；\
                缺条即虚报，拦截",
            SyncCode::BudgetOverspend => "预算超销被拒：核销累计不得超过 6,540；\
                请核对核销清单",
            SyncCode::DuplicateWriteOff => "重复核销被拒：同判据条只能核销一次；\
                请去重",
            SyncCode::LedgerRegress => "同步账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            SyncCode::EmptyVersion => "空版本被拒：版本串不得为空；请携带版本号",
            SyncCode::SyncFull => "同步对容量满：16 对上限；请先归档旧期",
            SyncCode::HandoffRewrite => "Handoff 后改写被拒：交接态只读；请走\
                勘误通道",
            SyncCode::HandoffBeforeAlign => "未对齐交接被拒：必须先 Aligned 再 \
                Handoff；请走状态机单向推进",
            SyncCode::NotReproducible => "双跑不一致：同输入两次结果不同；请\
                排查隐藏状态或非确定哈希",
            SyncCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x914C0..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [SyncCode; 12] = [
        SyncCode::OneSidedEntry,
        SyncCode::VersionDrift,
        SyncCode::MissingCriteria,
        SyncCode::BudgetOverspend,
        SyncCode::DuplicateWriteOff,
        SyncCode::LedgerRegress,
        SyncCode::EmptyVersion,
        SyncCode::SyncFull,
        SyncCode::HandoffRewrite,
        SyncCode::HandoffBeforeAlign,
        SyncCode::NotReproducible,
        SyncCode::SegmentViolation,
    ];
}

// ── 同步状态机 ────────────────────────────────────────────────────────
/// 同步状态：Collecting → Aligned → Handoff 单向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// 收集中
    Collecting,
    /// 报表↔handoff 版本已对齐
    Aligned,
    /// 已交接
    Handoff,
}

// ── 同步注册表 ────────────────────────────────────────────────────────
/// 一对版本登记。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SyncPair {
    /// 报表版本
    pub report: u32,
    /// handoff 版本
    pub handoff: u32,
}

/// 同步注册表：成对登记、逐对核齐。
pub struct SyncRegistry {
    pairs: Vec<SyncPair>,
    /// 当前状态
    pub state: SyncState,
}

impl SyncRegistry {
    /// 空注册表。
    pub fn new() -> SyncRegistry {
        SyncRegistry { pairs: Vec::new(), state: SyncState::Collecting }
    }

    /// 成对登记：两侧版本必须相等且非 0；容量 16。
    pub fn register(&mut self, report: u32, handoff: u32) -> Result<(), SyncCode> {
        if self.state == SyncState::Handoff {
            return Err(SyncCode::HandoffRewrite);
        }
        if report == 0 || handoff == 0 {
            return Err(SyncCode::EmptyVersion);
        }
        if self.pairs.len() >= SYNC_CAP {
            return Err(SyncCode::SyncFull);
        }
        if report != handoff {
            return Err(SyncCode::VersionDrift);
        }
        self.pairs.push(SyncPair { report, handoff });
        if !self.pairs.is_empty() {
            self.state = SyncState::Aligned;
        }
        Ok(())
    }

    /// 全部成对是否核齐（每对 report==handoff）。
    pub fn all_aligned(&self) -> bool {
        let mut i = 0usize;
        while i < self.pairs.len() {
            if let Some(p) = self.pairs.get(i) {
                if p.report != p.handoff {
                    return false;
                }
            }
            i += 1;
        }
        !self.pairs.is_empty()
    }

    /// 成对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// 交接：必须 Aligned；Handoff 后只读。
    pub fn handoff(&mut self) -> Result<(), SyncCode> {
        if self.state == SyncState::Collecting {
            return Err(SyncCode::HandoffBeforeAlign);
        }
        if !self.all_aligned() {
            return Err(SyncCode::VersionDrift);
        }
        self.state = SyncState::Handoff;
        Ok(())
    }
}

// ── 收口小结账 ────────────────────────────────────────────────────────
/// 收口小结账：20 条判据 + 预算核销。
pub struct SummaryLedger {
    /// 判据达成位图（true=达成并落账）
    criteria: [bool; CRITERIA_N],
    /// 已落账条数
    pub settled: usize,
    /// 核销累计
    pub spent: u32,
    /// 已核销条目位图
    written: [bool; CRITERIA_N],
    /// 核销条数
    pub writeoffs: usize,
}

impl SummaryLedger {
    /// 空账。
    pub fn new() -> SummaryLedger {
        SummaryLedger {
            criteria: [false; CRITERIA_N],
            settled: 0,
            spent: 0,
            written: [false; CRITERIA_N],
            writeoffs: 0,
        }
    }

    /// 落一条判据：序号越界/重复显性拒绝。
    pub fn settle(&mut self, idx: usize) -> Result<(), SyncCode> {
        if idx >= CRITERIA_N {
            return Err(SyncCode::MissingCriteria);
        }
        if let Some(slot) = self.criteria.get(idx) {
            if *slot {
                return Err(SyncCode::DuplicateWriteOff);
            }
        }
        if let Some(slot) = self.criteria.get_mut(idx) {
            *slot = true;
        }
        self.settled = self.settled.saturating_add(1);
        Ok(())
    }

    /// 20 条是否齐备。
    pub fn complete(&self) -> bool {
        self.settled == CRITERIA_N
    }

    /// 核销一笔：额度守恒（累计 ≤ 6,540），同条不双销。
    pub fn write_off(&mut self, idx: usize, amount: u32) -> Result<u32, SyncCode> {
        if idx >= CRITERIA_N {
            return Err(SyncCode::MissingCriteria);
        }
        if let Some(w) = self.written.get(idx) {
            if *w {
                return Err(SyncCode::DuplicateWriteOff);
            }
        }
        let next = self.spent.saturating_add(amount);
        if next > BUDGET {
            return Err(SyncCode::BudgetOverspend);
        }
        if let Some(w) = self.written.get_mut(idx) {
            *w = true;
        }
        self.spent = next;
        self.writeoffs = self.writeoffs.saturating_add(1);
        Ok(self.spent)
    }

    /// 预算核销守恒：全部 20 条核销后累计必须恰为 6,540。
    pub fn budget_conserved(&self) -> bool {
        self.complete() && self.spent == BUDGET
    }

    /// 核销守恒：核销额不超预算（部分核销期间）。
    pub fn within_budget(&self) -> bool {
        self.spent <= BUDGET
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏同步样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSyncCase {
    /// 版本漂移（report≠handoff）
    Drift,
    /// 判据缺条（只落 19 条）
    Missing,
    /// 预算超销
    Overspend,
    /// 重复核销
    DupWrite,
    /// 未对齐即交接
    EarlyHandoff,
}

impl BadSyncCase {
    /// 闭集全量。
    pub const ALL: [BadSyncCase; 5] = [
        BadSyncCase::Drift,
        BadSyncCase::Missing,
        BadSyncCase::Overspend,
        BadSyncCase::DupWrite,
        BadSyncCase::EarlyHandoff,
    ];

    /// 注入一轮：返回是否被显式检出。
    pub fn inject(self) -> bool {
        match self {
            BadSyncCase::Drift => {
                let mut r = SyncRegistry::new();
                r.register(5, 6) == Err(SyncCode::VersionDrift)
            }
            BadSyncCase::Missing => {
                let mut l = SummaryLedger::new();
                let mut i = 0usize;
                while i < CRITERIA_N - 1 {
                    let _ = l.settle(i);
                    i += 1;
                }
                !l.complete()
            }
            BadSyncCase::Overspend => {
                let mut l = SummaryLedger::new();
                let mut i = 0usize;
                let mut all_ok = true;
                while i < CRITERIA_N {
                    if l.settle(i).is_err() {
                        all_ok = false;
                    }
                    i += 1;
                }
                if !all_ok {
                    return false;
                }
                // 逐条核销：前 19 条各 330（=6,270），末条 271 → 累计 6,541 超销必拒
                let mut ok = true;
                let mut j = 0usize;
                while j < CRITERIA_N {
                    let amount = if j == CRITERIA_N - 1 { BUDGET - 330 * (CRITERIA_N as u32 - 1) + 1 } else { 330 };
                    if l.write_off(j, amount).is_err() {
                        ok = false;
                    }
                    j += 1;
                }
                !ok
            }
            BadSyncCase::DupWrite => {
                let mut l = SummaryLedger::new();
                let _ = l.settle(0);
                let _ = l.write_off(0, 300);
                l.write_off(0, 300) == Err(SyncCode::DuplicateWriteOff)
            }
            BadSyncCase::EarlyHandoff => {
                let mut r = SyncRegistry::new();
                r.handoff() == Err(SyncCode::HandoffBeforeAlign)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_sync_injection() -> ([u32; 3], [u32; 5]) {
    let mut injected = 0u32;
    let mut detected = 0u32;
    let mut escaped = 0u32;
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSyncCase::ALL.len() {
        if let Some(_case) = BadSyncCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = BadSyncCase::ALL.get(ci).copied().unwrap_or(BadSyncCase::Drift).inject();
                injected += 1;
                if caught {
                    detected += 1;
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    escaped += 1;
                }
                round += 1;
            }
        }
        ci += 1;
    }
    ([injected, detected, escaped], per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏账柄族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺条缺对族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；版本漂移族"),
    ("EOVERFLOW", -75, 141, "Windows ERROR_ARITHMETIC_OVERFLOW；预算超销族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；重复核销族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：登记→对齐→落账→核销→交接全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut r = SyncRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut v = 1u32;
    while v <= 4 {
        let reg = r.register(v, v);
        bytes.push(if reg.is_ok() { 1 } else { 0 });
        v += 1;
    }
    let drift = r.register(9, 8);
    bytes.push(if drift.is_err() { 2 } else { 3 });
    let mut l = SummaryLedger::new();
    let mut i = 0usize;
    while i < CRITERIA_N {
        let _ = l.settle(i);
        i += 1;
    }
    bytes.push(if l.complete() { 4 } else { 5 });
    // 20 条均摊核销：6,540 / 20 = 327 首条 300 + 19×327=6,513 → 300+6,513=6,813 超？
    // 均摊方案：首 19 条各 330（=6,270），末条 270 → 合计 6,540。
    let mut j = 0usize;
    while j < CRITERIA_N {
        let amount = if j < CRITERIA_N - 1 { 330 } else { BUDGET - 330 * (CRITERIA_N as u32 - 1) };
        match l.write_off(j, amount) {
            Ok(_) => bytes.push(6),
            Err(_) => bytes.push(7),
        }
        j += 1;
    }
    bytes.push(if l.budget_conserved() { 8 } else { 9 });
    let ho = r.handoff();
    bytes.push(if ho.is_ok() { 10 } else { 11 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10019 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10019_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10019");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x914C0..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x914C0..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 版本对齐 10/10：成对等版本登记全收
    let mut aligned = 0u32;
    let mut n = 0u32;
    while n < INJECT_ROUNDS {
        let mut r = SyncRegistry::new();
        if r.register(100 + n, 100 + n).is_ok() {
            aligned += 1;
        }
        n += 1;
    }
    s.add("同步留痕：报表版本↔handoff 版本对齐 10/10", aligned == INJECT_ROUNDS,
          "成对等版本登记全部接受（判据原文）");

    // 3. 版本漂移 10/10 检出
    let mut drift_hits = 0u32;
    n = 0;
    while n < INJECT_ROUNDS {
        let mut r = SyncRegistry::new();
        if r.register(200 + n, 299 + n) == Err(SyncCode::VersionDrift) {
            drift_hits += 1;
        }
        n += 1;
    }
    s.add("漂移检出：单侧漂移版本 10/10 次检出", drift_hits == INJECT_ROUNDS,
          "report≠handoff 的成对登记必须全部拒绝");

    // 4. 20 条判据落账：逐条全落
    let mut l = SummaryLedger::new();
    let mut i = 0usize;
    let mut settled_ok = true;
    while i < CRITERIA_N {
        if l.settle(i).is_err() {
            settled_ok = false;
        }
        i += 1;
    }
    s.add("收口小结：20 条判据逐条落账齐备", settled_ok && l.complete(),
          "20 条判据达成是收口前置（判据原文）");

    // 5. 缺条即虚报：19 条不齐
    let mut l2 = SummaryLedger::new();
    let mut i2 = 0usize;
    while i2 < CRITERIA_N - 1 {
        let _ = l2.settle(i2);
        i2 += 1;
    }
    s.add("虚报拦截：19/20 条不齐判虚报", !l2.complete(),
          "缺条即虚报，收口拒绝");

    // 6. 预算核销守恒：20 条均摊恰 6,540
    let mut l3 = SummaryLedger::new();
    let mut i3 = 0usize;
    while i3 < CRITERIA_N {
        let _ = l3.settle(i3);
        i3 += 1;
    }
    let mut spent_ok = true;
    let mut j = 0usize;
    while j < CRITERIA_N {
        let amount = if j < CRITERIA_N - 1 { 330 } else { BUDGET - 330 * (CRITERIA_N as u32 - 1) };
        if l3.write_off(j, amount).is_err() {
            spent_ok = false;
        }
        j += 1;
    }
    s.add("预算核销：20 条核销累计恰为 6,540 零偏离", spent_ok && l3.budget_conserved(),
          "核销累计必须等于冻结预算 6,540（判据原文）");

    // 7. 核销不符检出：超销被拦
    let mut l4 = SummaryLedger::new();
    let mut i4 = 0usize;
    while i4 < CRITERIA_N {
        let _ = l4.settle(i4);
        i4 += 1;
    }
    let overspend = l4.write_off(0, BUDGET + 1) == Err(SyncCode::BudgetOverspend);
    s.add("核销不符：单笔超预算被拦", overspend,
          "核销额超出 6,540 的任何一笔都必须拒绝");

    // 8. 重复核销拦截
    let mut l5 = SummaryLedger::new();
    let _ = l5.settle(0);
    let _ = l5.write_off(0, 300);
    let dup = l5.write_off(0, 300) == Err(SyncCode::DuplicateWriteOff);
    s.add("防重：同条二次核销被拒", dup,
          "每条判据只核销一次");

    // 9. 状态机：未对齐交接被拒，对齐后交接成功
    let mut r = SyncRegistry::new();
    let early = r.handoff() == Err(SyncCode::HandoffBeforeAlign);
    let _ = r.register(7, 7);
    let late = r.handoff().is_ok() && r.state == SyncState::Handoff;
    s.add("状态机：Collecting 交接被拒、Aligned 交接成功", early && late,
          "交接必须在版本对齐之后");

    // 10. Handoff 后改写被拒
    let rewrite = r.register(8, 8) == Err(SyncCode::HandoffRewrite);
    s.add("状态机：Handoff 态只读", rewrite,
          "交接后任何登记都显性拒绝");

    // 11. 注入核：五类坏样本全检出
    let (totals, per_class) = run_sync_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "漂移/缺条/超销/双销/早交接全被显式拒绝");

    // 12. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          totals.get(0).copied().unwrap_or(0) == totals.get(1).copied().unwrap_or(0) + totals.get(2).copied().unwrap_or(0)
              && totals.get(2).copied().unwrap_or(1) == 0
              && totals.get(0).copied().unwrap_or(0) == 5 * INJECT_ROUNDS,
          "50 次注入必须 50 次检出、0 逃逸");

    // 13. 诊断码闭集：文案非空
    let text_ok = SyncCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i5 = 0usize;
    while i5 < SyncCode::ALL.len() {
        let mut j5 = i5 + 1;
        while j5 < SyncCode::ALL.len() {
            if let (Some(x), Some(y)) = (SyncCode::ALL.get(i5), SyncCode::ALL.get(j5)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j5 += 1;
        }
        i5 += 1;
    }
    let in_seg = SyncCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(EOVERFLOW)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 17. 空版本拒绝
    let mut r2 = SyncRegistry::new();
    let zero = r2.register(0, 0) == Err(SyncCode::EmptyVersion);
    s.add("边界：版本 0 拒绝登记", zero,
          "版本号从 1 起，0 号无法对账");

    // 动态明细（≤8）：注入账一行 + 指纹一行
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        totals.get(0).copied().unwrap_or(0),
        totals.get(1).copied().unwrap_or(0),
        totals.get(2).copied().unwrap_or(0)
    );
    s.add_dyn("账目快照", totals.get(2).copied().unwrap_or(1) == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_pair_accepted() {
        let mut r = SyncRegistry::new();
        assert!(r.register(3, 3).is_ok());
        assert_eq!(r.state, SyncState::Aligned);
    }

    #[test]
    fn drift_pair_rejected() {
        let mut r = SyncRegistry::new();
        assert_eq!(r.register(3, 4), Err(SyncCode::VersionDrift));
    }

    #[test]
    fn zero_version_rejected() {
        let mut r = SyncRegistry::new();
        assert_eq!(r.register(0, 0), Err(SyncCode::EmptyVersion));
    }

    #[test]
    fn handoff_requires_alignment() {
        let mut r = SyncRegistry::new();
        assert_eq!(r.handoff(), Err(SyncCode::HandoffBeforeAlign));
        let _ = r.register(1, 1);
        assert!(r.handoff().is_ok());
        assert_eq!(r.register(2, 2), Err(SyncCode::HandoffRewrite));
    }

    #[test]
    fn summary_needs_all_20() {
        let mut l = SummaryLedger::new();
        let mut i = 0usize;
        while i < 19 {
            let _ = l.settle(i);
            i += 1;
        }
        assert!(!l.complete());
        let _ = l.settle(19);
        assert!(l.complete());
    }

    #[test]
    fn budget_conserved_6540() {
        let mut l = SummaryLedger::new();
        let mut i = 0usize;
        while i < CRITERIA_N {
            let _ = l.settle(i);
            i += 1;
        }
        let mut j = 0usize;
        while j < CRITERIA_N {
            let amount = if j < CRITERIA_N - 1 { 330 } else { BUDGET - 330 * 19 };
            assert!(l.write_off(j, amount).is_ok());
            j += 1;
        }
        assert_eq!(l.spent, BUDGET);
        assert!(l.budget_conserved());
    }

    #[test]
    fn overspend_blocked() {
        let mut l = SummaryLedger::new();
        let _ = l.settle(0);
        assert_eq!(l.write_off(0, BUDGET + 1), Err(SyncCode::BudgetOverspend));
    }

    #[test]
    fn duplicate_writeoff_blocked() {
        let mut l = SummaryLedger::new();
        let _ = l.settle(0);
        let _ = l.write_off(0, 100);
        assert_eq!(l.write_off(0, 100), Err(SyncCode::DuplicateWriteOff));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (totals, per) = run_sync_injection();
        assert_eq!(totals.get(2).copied(), Some(0));
        assert_eq!(totals.get(0).copied(), Some(5 * INJECT_ROUNDS));
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EOVERFLOW));
        assert!(agrees_with_linux(EEXIST));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10019_checks_green() {
        let s = run_unx10019_checks();
        assert!(s.all_passed(), "UNX-F10019 red: {}", s.red_items_text());
    }
}
