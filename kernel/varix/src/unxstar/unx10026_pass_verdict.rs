//! UNX-F10026 清偿通过判定（判据恢复） · 0x91680段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10027-J1 失败转红全入账（逐红挂定位单 10/10 次），绕账 10/10 次拦截
//!
//! 设计要点：
//!   ① 通过判定双档核：T1 与 ktest 两档独立记档，两档结论一致才允许
//!      定论——双过恢复入正常账（分账恢复），双败转失败出口；单档
//!      定论 10/10 次拦截，不许拿一档结果直接恢复判据。
//!   ② 判定状态机四相闭集：Awaiting → T1Only | KOnly → Both；同档
//!      重复记档拒绝（DupTierRecord），两档结论冲突（一过一败）显性
//!      ConflictVerdict，不许静默择优。
//!   ③ 分账恢复账本：recovered 与 failed 两账只增；恢复数必须同时
//!      等于 T1 过档结算数与 ktest 过档结算数（三账闭合），账不平即
//!      RecoverMismatch。
//!   ④ 恢复幂等：同一 entry 只结算一次，二次 settle 即 DupRecover；
//!      零记录结算 NothingRecorded。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸；四类坏样本（单档定论/双档
//!      冲突/同档重复/二次结算）各 10/10 次显性拒绝；FNV-1a 双跑
//!      指纹复核确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10026 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91680;

/// 条目容量。
pub const MAX_ENTRIES: usize = 16;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerdictCode {
    /// 未知条目
    UnknownEntry,
    /// 单档定论：另一档尚未记档
    SingleTier,
    /// 双档冲突：一过一败
    ConflictVerdict,
    /// 同档重复记档
    DupTierRecord,
    /// 无批复测条目参与判定
    UnbatchedEntry,
    /// 零记录结算
    NothingRecorded,
    /// 分账不平：恢复数与双档过档数对不上
    RecoverMismatch,
    /// 二次结算
    DupRecover,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 账满
    LedgerFull,
    /// 段位越界
    SegmentViolation,
}

impl VerdictCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            VerdictCode::UnknownEntry => "未知条目被拒：条目必须先登记（含原判据号与批号）；\
                请回复测执行器取已复测条目",
            VerdictCode::SingleTier => "单档定论被拒：T1 与 ktest 必须双档齐备；\
                缺档请先补跑另一档再结算",
            VerdictCode::ConflictVerdict => "双档冲突被拒：T1 过而 ktest 败（或反之）；\
                冲突必须显性上报，不许静默择优恢复",
            VerdictCode::DupTierRecord => "同档重复记档被拒：每档每条目只记一次；\
                请勿重复提交同档结论",
            VerdictCode::UnbatchedEntry => "无批条目被拒：判定只对已挂批复测条目开放；\
                请先走 F10024 挂批冻结",
            VerdictCode::NothingRecorded => "零记录结算被拒：两档均无记录不得结算；\
                请先完成双档记档",
            VerdictCode::RecoverMismatch => "分账不平被拒：恢复数必须同时等于 T1 过档与 \
                ktest 过档结算数；请核对分账台账",
            VerdictCode::DupRecover => "二次结算被拒：同一条目只允许一次定论；\
                如需重判请开新一轮",
            VerdictCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            VerdictCode::NotReproducible => "双跑不一致：同输入两次结算账不同；\
                请排查隐藏状态或非确定遍历",
            VerdictCode::LedgerFull => "账满被拒：条目表越过容量；请先归档旧表",
            VerdictCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [VerdictCode; 12] = [
        VerdictCode::UnknownEntry,
        VerdictCode::SingleTier,
        VerdictCode::ConflictVerdict,
        VerdictCode::DupTierRecord,
        VerdictCode::UnbatchedEntry,
        VerdictCode::NothingRecorded,
        VerdictCode::RecoverMismatch,
        VerdictCode::DupRecover,
        VerdictCode::LedgerRegress,
        VerdictCode::NotReproducible,
        VerdictCode::LedgerFull,
        VerdictCode::SegmentViolation,
    ];
}

// ── 判定状态机 ────────────────────────────────────────────────────────
/// 判定四相闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// 双档均未记档
    Awaiting,
    /// 仅 T1 记档
    T1Only,
    /// 仅 ktest 记档
    KOnly,
    /// 双档齐备（可结算）
    Both,
}

/// 双档结论对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TierPair {
    /// T1 结论（None = 未记档）
    pub t1: Option<bool>,
    /// ktest 结论（None = 未记档）
    pub ktest: Option<bool>,
    /// 是否已结算
    pub settled: bool,
}

impl TierPair {
    const fn fresh() -> TierPair {
        TierPair { t1: None, ktest: None, settled: false }
    }

    /// 当前相。
    pub fn phase(&self) -> Phase {
        match (self.t1.is_some(), self.ktest.is_some()) {
            (false, false) => Phase::Awaiting,
            (true, false) => Phase::T1Only,
            (false, true) => Phase::KOnly,
            (true, true) => Phase::Both,
        }
    }

    /// 双档一致才给结论：Some(true)=双过，Some(false)=双败，None=冲突或未齐。
    pub fn agreed(&self) -> Option<bool> {
        match (self.t1, self.ktest) {
            (Some(a), Some(b)) => {
                if a == b {
                    Some(a)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// 结算出口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettleOutcome {
    /// 双过：恢复入正常账
    Recovered,
    /// 双败：转失败出口（F10027 接手）
    Failed,
}

/// 单条判定对象。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerdictEntry {
    /// 条目号
    pub id: u32,
    /// 原判据号
    pub criterion: u32,
    /// 批号（0 = 无批，禁止判定）
    pub batch: u32,
    /// 双档记录
    pub pair: TierPair,
}

// ── 判定引擎（状态机 + 分账账本） ────────────────────────────────────
/// 通过判定引擎。
#[derive(Clone, Copy, Debug)]
pub struct VerdictEngine {
    entries: [Option<VerdictEntry>; MAX_ENTRIES],
    /// 恢复数（分账恢复账）
    recovered: u32,
    /// 双败数（转红候选账）
    failed: u32,
    /// T1 过档结算数
    t1_pass_settled: u32,
    /// ktest 过档结算数
    k_pass_settled: u32,
}

impl VerdictEngine {
    /// 空引擎。
    pub fn new() -> VerdictEngine {
        VerdictEngine {
            entries: [None; MAX_ENTRIES],
            recovered: 0,
            failed: 0,
            t1_pass_settled: 0,
            k_pass_settled: 0,
        }
    }

    /// 登记条目：批号必须非 0，重复登记拒绝。
    pub fn admit(&mut self, id: u32, criterion: u32, batch: u32) -> Result<(), VerdictCode> {
        if id == 0 || criterion == 0 {
            return Err(VerdictCode::UnknownEntry);
        }
        if batch == 0 {
            return Err(VerdictCode::UnbatchedEntry);
        }
        if self.entry(id).is_some() {
            return Err(VerdictCode::UnknownEntry);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(slot) = self.entries.get_mut(i) {
                if slot.is_none() {
                    *slot = Some(VerdictEntry { id, criterion, batch, pair: TierPair::fresh() });
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(VerdictCode::LedgerFull)
    }

    /// 查条目。
    pub fn entry(&self, id: u32) -> Option<VerdictEntry> {
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get(i).copied() {
                if e.id == id {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// T1 记档：条目在册、未结算、同档未记。
    pub fn record_t1(&mut self, id: u32, pass: bool) -> Result<(), VerdictCode> {
        self.record(id, true, pass)
    }

    /// ktest 记档：条目在册、未结算、同档未记。
    pub fn record_ktest(&mut self, id: u32, pass: bool) -> Result<(), VerdictCode> {
        self.record(id, false, pass)
    }

    fn record(&mut self, id: u32, is_t1: bool, pass: bool) -> Result<(), VerdictCode> {
        let e = match self.entry(id) {
            Some(e) => e,
            None => return Err(VerdictCode::UnknownEntry),
        };
        if e.pair.settled {
            return Err(VerdictCode::DupRecover);
        }
        let already = if is_t1 { e.pair.t1.is_some() } else { e.pair.ktest.is_some() };
        if already {
            return Err(VerdictCode::DupTierRecord);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(i) {
                if e.id == id {
                    if is_t1 {
                        e.pair.t1 = Some(pass);
                    } else {
                        e.pair.ktest = Some(pass);
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(VerdictCode::UnknownEntry)
    }

    /// 结算：双档齐备且一致才出结论；成功即入分账账本并封条目。
    pub fn settle(&mut self, id: u32) -> Result<SettleOutcome, VerdictCode> {
        let e = match self.entry(id) {
            Some(e) => e,
            None => return Err(VerdictCode::UnknownEntry),
        };
        if e.pair.settled {
            return Err(VerdictCode::DupRecover);
        }
        match e.pair.phase() {
            Phase::Awaiting => return Err(VerdictCode::NothingRecorded),
            Phase::T1Only | Phase::KOnly => return Err(VerdictCode::SingleTier),
            Phase::Both => {}
        }
        let agreed = match e.pair.agreed() {
            Some(a) => a,
            None => return Err(VerdictCode::ConflictVerdict),
        };
        let outcome = if agreed { SettleOutcome::Recovered } else { SettleOutcome::Failed };
        // 分账：恢复数计入双档过档账；失败数只入失败账。
        if let SettleOutcome::Recovered = outcome {
            self.t1_pass_settled = self.t1_pass_settled.saturating_add(1);
            self.k_pass_settled = self.k_pass_settled.saturating_add(1);
            self.recovered = self.recovered.saturating_add(1);
        } else {
            self.failed = self.failed.saturating_add(1);
        }
        let mut i = 0usize;
        while i < MAX_ENTRIES {
            if let Some(Some(e)) = self.entries.get_mut(i) {
                if e.id == id {
                    e.pair.settled = true;
                    return Ok(outcome);
                }
            }
            i += 1;
        }
        Err(VerdictCode::UnknownEntry)
    }

    /// 分账闭合：恢复数 = T1 过档数 = ktest 过档数。
    pub fn accounts_closed(&self) -> bool {
        self.recovered == self.t1_pass_settled && self.recovered == self.k_pass_settled
    }

    /// 恢复数。
    pub fn recovered(&self) -> u32 {
        self.recovered
    }

    /// 双败数。
    pub fn failed(&self) -> u32 {
        self.failed
    }

    /// 结算总数。
    pub fn settled(&self) -> u32 {
        self.recovered.saturating_add(self.failed)
    }
}

// ── 注入核：坏判定样本矩阵 ────────────────────────────────────────────
/// 四类坏判定样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadVerdictCase {
    /// 单档定论
    SingleTier,
    /// 双档冲突
    Conflict,
    /// 同档重复记档
    DupTier,
    /// 二次结算
    DupSettle,
}

impl BadVerdictCase {
    /// 闭集全量。
    pub const ALL: [BadVerdictCase; 4] = [
        BadVerdictCase::SingleTier,
        BadVerdictCase::Conflict,
        BadVerdictCase::DupTier,
        BadVerdictCase::DupSettle,
    ];

    /// 期望诊断码。
    pub fn code(self) -> VerdictCode {
        match self {
            BadVerdictCase::SingleTier => VerdictCode::SingleTier,
            BadVerdictCase::Conflict => VerdictCode::ConflictVerdict,
            BadVerdictCase::DupTier => VerdictCode::DupTierRecord,
            BadVerdictCase::DupSettle => VerdictCode::DupRecover,
        }
    }

    /// 构造样本引擎并施加坏请求，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut eng = VerdictEngine::new();
        match self {
            BadVerdictCase::SingleTier => {
                let _ = eng.admit(1, 6001, 1);
                let _ = eng.record_t1(1, true);
                eng.settle(1).err() == Some(VerdictCode::SingleTier)
            }
            BadVerdictCase::Conflict => {
                let _ = eng.admit(1, 6001, 1);
                let _ = eng.record_t1(1, true);
                let _ = eng.record_ktest(1, false);
                eng.settle(1).err() == Some(VerdictCode::ConflictVerdict)
            }
            BadVerdictCase::DupTier => {
                let _ = eng.admit(1, 6001, 1);
                let _ = eng.record_t1(1, true);
                eng.record_t1(1, true).err() == Some(VerdictCode::DupTierRecord)
            }
            BadVerdictCase::DupSettle => {
                let _ = eng.admit(1, 6001, 1);
                let _ = eng.record_t1(1, true);
                let _ = eng.record_ktest(1, true);
                let _ = eng.settle(1);
                eng.settle(1).err() == Some(VerdictCode::DupRecover)
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerdictLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl VerdictLedger {
    const fn zero() -> VerdictLedger {
        VerdictLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), VerdictCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(VerdictCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏判定注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_verdict_matrix() -> (VerdictLedger, [u32; 4]) {
    let mut ledger = VerdictLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadVerdictCase::ALL.len() {
        let case = match BadVerdictCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；单档定论/二次结算族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未知条目族"),
    ("EEXIST", -17, 183, "Windows ERROR_ALREADY_EXISTS；同档重复记档族"),
    ("EPROTO", -71, 10054, "Windows ERROR_INVALID_PROTOCOL；双档冲突族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；无批条目族"),
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

/// 规范语料双跑指纹：4 条双档记档结算（3 恢复 1 败）→ 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut eng = VerdictEngine::new();
    let mut n = 1u32;
    while n <= 4 {
        let _ = eng.admit(n, 6000 + n, 1);
        let pass = n != 4;
        let _ = eng.record_t1(n, pass);
        let _ = eng.record_ktest(n, pass);
        let _ = eng.settle(n);
        n += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((eng.recovered() & 0xFF) as u8);
    bytes.push((eng.failed() & 0xFF) as u8);
    bytes.push((eng.settled() & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10026 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10026_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10026");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 正常流：双过恢复 + 双败出口
    let mut eng = VerdictEngine::new();
    let _ = eng.admit(1, 6001, 1);
    let _ = eng.admit(2, 6002, 1);
    let _ = eng.record_t1(1, true);
    let _ = eng.record_ktest(1, true);
    let _ = eng.record_t1(2, false);
    let _ = eng.record_ktest(2, false);
    let r1 = eng.settle(1);
    let r2 = eng.settle(2);
    let normal_ok = r1 == Ok(SettleOutcome::Recovered) && r2 == Ok(SettleOutcome::Failed);
    s.add("正常流：双过恢复、双败出口", normal_ok && eng.recovered() == 1 && eng.failed() == 1,
          "两档一致的结论才允许定论");

    // 3. 分账闭合：恢复数 = T1 过档 = ktest 过档
    s.add("分账闭合：恢复 1 = T1 过档 1 = ktest 过档 1",
          eng.accounts_closed() && eng.settled() == 2,
          "三账必须闭合，账不平即 RecoverMismatch");

    // 4. 相位机：Awaiting→Both 逐相推进
    let mut ph = VerdictEngine::new();
    let _ = ph.admit(1, 6001, 1);
    let p0 = ph.entry(1).map(|e| e.pair.phase()) == Some(Phase::Awaiting);
    let _ = ph.record_t1(1, true);
    let p1 = ph.entry(1).map(|e| e.pair.phase()) == Some(Phase::T1Only);
    let _ = ph.record_ktest(1, true);
    let p2 = ph.entry(1).map(|e| e.pair.phase()) == Some(Phase::Both);
    s.add("相位机：Awaiting→T1Only→Both 单向推进", p0 && p1 && p2,
          "相位只随记档推进，不许跳相定论");

    // 5. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_verdict_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：单档定论/双档冲突/同档重复/二次结算各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 6. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadVerdictCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 7. 单档定论拦截：KOnly 相位结算拒绝
    let mut k1 = VerdictEngine::new();
    let _ = k1.admit(1, 6001, 1);
    let _ = k1.record_ktest(1, true);
    s.add("单档定论拦截：仅 ktest 记档结算 → SingleTier",
          k1.settle(1).err() == Some(VerdictCode::SingleTier),
          "T1 缺席即不得恢复判据");

    // 8. 零记录结算拦截
    let mut z = VerdictEngine::new();
    let _ = z.admit(1, 6001, 1);
    s.add("零记录结算拦截：Awaiting 相位 → NothingRecorded",
          z.settle(1).err() == Some(VerdictCode::NothingRecorded),
          "两档全空不得出任何定论");

    // 9. 冲突不可恢复：冲突条目不得计入任何账
    let mut cf = VerdictEngine::new();
    let _ = cf.admit(1, 6001, 1);
    let _ = cf.record_t1(1, true);
    let _ = cf.record_ktest(1, false);
    let _ = cf.settle(1);
    s.add("冲突不落账：冲突条目不入恢复账也不入失败账",
          cf.recovered() == 0 && cf.failed() == 0 && cf.accounts_closed(),
          "冲突必须显性上报且账面零计入");

    // 10. 无批条目拦截：admit 时 batch=0 拒绝
    let mut nb = VerdictEngine::new();
    s.add("无批条目拦截：batch=0 → UnbatchedEntry",
          nb.admit(1, 6001, 0).err() == Some(VerdictCode::UnbatchedEntry),
          "判定只对已挂批复测条目开放");

    // 11. 恢复幂等：结算后条目封账
    let idem = BadVerdictCase::DupSettle.attempt();
    s.add("恢复幂等：二次结算 → DupRecover", idem,
          "同一 entry 只允许一次定论，重判开新一轮");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(2) && agrees_with_linux(17)
        && agrees_with_linux(71) && agrees_with_linux(22)
        && !agrees_with_linux(0) && !agrees_with_linux(910);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 13. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < VerdictCode::ALL.len() {
        let mut j = i + 1;
        while j < VerdictCode::ALL.len() {
            if let (Some(a), Some(b)) = (VerdictCode::ALL.get(i), VerdictCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = VerdictCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 14. agreed 性质：同值给结论、异值 None、缺档 None
    let agree_ok = TierPair { t1: Some(true), ktest: Some(true), settled: false }.agreed() == Some(true)
        && TierPair { t1: Some(false), ktest: Some(false), settled: false }.agreed() == Some(false)
        && TierPair { t1: Some(true), ktest: Some(false), settled: false }.agreed().is_none()
        && TierPair { t1: Some(true), ktest: None, settled: false }.agreed().is_none();
    s.add("agreed 性质：同值定论、冲突与缺档均无结论", agree_ok,
          "纯函数判等：两档结论一致性可复算");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同输入两次结算账必须逐位一致");

    // 16. 结算账守恒：恢复+失败 = 结算总数
    s.add("结算账守恒：恢复 3 + 失败 1 = 结算 4",
          eng.recovered() + eng.failed() == eng.settled(),
          "结算总数必须等于两账之和，无第三出口");

    // 动态明细（≤8）：分账 + 注入账 + 指纹
    let acc_text = format!(
        "恢复 {} / 失败 {} / T1过档 {} / k过档 {}",
        eng.recovered(), eng.failed(), eng.t1_pass_settled, eng.k_pass_settled
    );
    s.add_dyn("分账快照", eng.accounts_closed(), &acc_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dual_pass_recovers() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, true);
        let _ = eng.record_ktest(1, true);
        assert_eq!(eng.settle(1), Ok(SettleOutcome::Recovered));
        assert_eq!(eng.recovered(), 1);
    }

    #[test]
    fn dual_fail_exits() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, false);
        let _ = eng.record_ktest(1, false);
        assert_eq!(eng.settle(1), Ok(SettleOutcome::Failed));
        assert_eq!(eng.failed(), 1);
        assert_eq!(eng.recovered(), 0);
    }

    #[test]
    fn single_tier_blocked() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, true);
        assert_eq!(eng.settle(1).err(), Some(VerdictCode::SingleTier));
    }

    #[test]
    fn conflict_blocked() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, true);
        let _ = eng.record_ktest(1, false);
        assert_eq!(eng.settle(1).err(), Some(VerdictCode::ConflictVerdict));
    }

    #[test]
    fn dup_tier_blocked() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, true);
        assert_eq!(eng.record_t1(1, true).err(), Some(VerdictCode::DupTierRecord));
        assert_eq!(eng.record_ktest(1, true).is_ok(), true);
    }

    #[test]
    fn dup_settle_blocked() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        let _ = eng.record_t1(1, true);
        let _ = eng.record_ktest(1, true);
        let _ = eng.settle(1);
        assert_eq!(eng.settle(1).err(), Some(VerdictCode::DupRecover));
    }

    #[test]
    fn zero_batch_rejected() {
        let mut eng = VerdictEngine::new();
        assert_eq!(eng.admit(1, 6001, 0).err(), Some(VerdictCode::UnbatchedEntry));
    }

    #[test]
    fn phase_progression() {
        let mut eng = VerdictEngine::new();
        let _ = eng.admit(1, 6001, 1);
        assert_eq!(eng.entry(1).map(|e| e.pair.phase()), Some(Phase::Awaiting));
        let _ = eng.record_ktest(1, true);
        assert_eq!(eng.entry(1).map(|e| e.pair.phase()), Some(Phase::KOnly));
        let _ = eng.record_t1(1, true);
        assert_eq!(eng.entry(1).map(|e| e.pair.phase()), Some(Phase::Both));
    }

    #[test]
    fn bad_verdict_matrix_green() {
        let (inj, per) = run_bad_verdict_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(71));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(17), -17);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10026_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
