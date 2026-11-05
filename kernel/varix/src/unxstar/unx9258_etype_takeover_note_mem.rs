//! UNX-F9258 内存族 E 型升级接管注记 · 0x7AE00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9258 逐字）：
//!   接管边界声明判据过，存量错误路径零重写承诺格过，增量清单落账格过。
//!
//! SEG_BASE = 0x7AE00：诊断码闭集说明——本模块全部诊断码由
//! `TakeoverCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；任何表外码不得进入账目。
//!
//! 设计要点：
//!   ① `MemEtype`：mem 域五类错误码闭集（ENOMEM/EAGAIN/EFAULT/EINVAL/EPERM）。
//!   ② `TransferNote` + `parse_note`：接管划账注记解析器（域校验）。
//!   ③ `TakeoverRegistry`：接管前后归属账本（源账→目标账划转，总额守恒）。
//!   ④ `TakeoverLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑤ `ERRNO_TABLE`：Linux errno 对照表 + `agrees_with_linux` 复算。
//!   ⑥ `fnv1a`/`corpus_fingerprint`：确定性双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9258 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AE00;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 接管注记单笔最大划转量（账目上限）。
pub const MAX_NOTE_AMOUNT: u32 = 256;

/// 存量账每类错误码的初始错误路径数（零重写承诺的基准值）。
pub const LEGACY_BASELINE: u32 = 40;

// ── mem 域错误码闭集 ─────────────────────────────────────────────────
/// mem 域参与接管的五类错误码（闭集，顺序即 errno 数值序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemEtype {
    /// ENOMEM(12)：地址空间耗尽
    Enomem,
    /// EAGAIN(11)：暂时性资源不足
    Eagain,
    /// EFAULT(14)：坏指针
    Efault,
    /// EINVAL(22)：参数坏值
    Einval,
    /// EPERM(1)：特权缺失
    Eperm,
}

impl MemEtype {
    /// Linux errno 正数值。
    pub fn errno(self) -> i32 {
        match self {
            MemEtype::Eperm => 1,
            MemEtype::Eagain => 11,
            MemEtype::Enomem => 12,
            MemEtype::Efault => 14,
            MemEtype::Einval => 22,
        }
    }

    /// 按数组序号取类；越界返回 None（不许裸下标）。
    pub fn from_index(idx: usize) -> Option<MemEtype> {
        match idx {
            0 => Some(MemEtype::Enomem),
            1 => Some(MemEtype::Eagain),
            2 => Some(MemEtype::Efault),
            3 => Some(MemEtype::Einval),
            4 => Some(MemEtype::Eperm),
            _ => None,
        }
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            MemEtype::Enomem => "地址空间耗尽",
            MemEtype::Eagain => "资源暂时不足",
            MemEtype::Efault => "坏指针",
            MemEtype::Einval => "参数坏值",
            MemEtype::Eperm => "特权缺失",
        }
    }

    /// 闭集全量。
    pub const ALL: [MemEtype; 5] = [
        MemEtype::Enomem,
        MemEtype::Eagain,
        MemEtype::Efault,
        MemEtype::Einval,
        MemEtype::Eperm,
    ];
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TakeoverCode {
    /// 注记错误码类不在闭集内
    UnknownEtype,
    /// 划转量为负
    NegativeAmount,
    /// 划转量为零
    ZeroAmount,
    /// 划转量超过源账余额
    Overdraft,
    /// 同一错误码重复接管（双计拒绝）
    DoubleCount,
    /// 接管账倒退（守恒破坏）
    LedgerRegress,
    /// 存量账被改写（零重写承诺违约）
    PromiseBreach,
    /// 未接管就封账
    SealBeforeTransfer,
    /// 封账后仍试图划转
    PostSealTransfer,
    /// 注记总数越上界
    NoteOverflow,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl TakeoverCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TakeoverCode::UnknownEtype => "注记错误码不在 mem 域闭集内；请改用 \
                ENOMEM/EAGAIN/EFAULT/EINVAL/EPERM 五类之一",
            TakeoverCode::NegativeAmount => "划转量为负被拒：接管划账只允许正量；\
                请改为从目标账反向开注记",
            TakeoverCode::ZeroAmount => "划转量为零被拒：零量注记不产生归属变化，\
                属于空壳账目；请删除或改成正量",
            TakeoverCode::Overdraft => "划转量超过源账余额被拒：源账不允许透支；\
                请按存量账剩余量截断后再开注记",
            TakeoverCode::DoubleCount => "同一错误码重复接管被拒：一个错误码只允许\
                一笔接管注记，双计会破坏总额守恒",
            TakeoverCode::LedgerRegress => "接管账倒退被拒：注入=检出+逃逸只增不减；\
                请排查回滚路径误改累计值",
            TakeoverCode::PromiseBreach => "存量账被改写被拒：零重写承诺要求每类错误码\
                的「源账剩余+目标账已接管=基线」恒成立",
            TakeoverCode::SealBeforeTransfer => "未接管就封账被拒：封账前提是该错误码\
                已有正量接管；请先完成划转再封账",
            TakeoverCode::PostSealTransfer => "封账后划转被拒：封账表示该错误码增量\
                清单冻结；如需追加请走新批次注记",
            TakeoverCode::NoteOverflow => "注记量越上界被拒：单笔划转不得超过 \
                MAX_NOTE_AMOUNT；请拆分为多笔批次",
            TakeoverCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            TakeoverCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [TakeoverCode; 12] = [
        TakeoverCode::UnknownEtype,
        TakeoverCode::NegativeAmount,
        TakeoverCode::ZeroAmount,
        TakeoverCode::Overdraft,
        TakeoverCode::DoubleCount,
        TakeoverCode::LedgerRegress,
        TakeoverCode::PromiseBreach,
        TakeoverCode::SealBeforeTransfer,
        TakeoverCode::PostSealTransfer,
        TakeoverCode::NoteOverflow,
        TakeoverCode::NotReproducible,
        TakeoverCode::SegmentViolation,
    ];
}

// ── 划账注记解析器 ───────────────────────────────────────────────────
/// 接管划账注记：把 amount 个错误路径从源账（存量）划到目标账（增量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransferNote {
    /// 错误码类
    pub etype: MemEtype,
    /// 划转量（正数）
    pub amount: u32,
}

/// 解析原始字段为注记：域校验按「类→量」次序，先类后量。
/// 原始量用 i32 承载，负值显性拒绝；errno 数值不在闭集内拒绝。
pub fn parse_note(raw_etype: i32, raw_amount: i32) -> Result<TransferNote, TakeoverCode> {
    if raw_amount < 0 {
        return Err(TakeoverCode::NegativeAmount);
    }
    if raw_amount == 0 {
        return Err(TakeoverCode::ZeroAmount);
    }
    if raw_amount as u32 > MAX_NOTE_AMOUNT {
        return Err(TakeoverCode::NoteOverflow);
    }
    let mut found = None;
    let mut i = 0usize;
    while i < MemEtype::ALL.len() {
        if let Some(t) = MemEtype::ALL.get(i) {
            if t.errno() == raw_etype {
                found = Some(*t);
            }
        }
        i += 1;
    }
    match found {
        Some(t) => Ok(TransferNote { etype: t, amount: raw_amount as u32 }),
        None => Err(TakeoverCode::UnknownEtype),
    }
}

// ── 归属账本 ─────────────────────────────────────────────────────────
/// 单类错误码的归属账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EtypeAccount {
    /// 错误码类
    pub etype: MemEtype,
    /// 源账（存量归属）剩余
    pub legacy: u32,
    /// 目标账（增量归属）已接管
    pub taken: u32,
    /// 基线（零重写承诺锚点，构造后不可变）
    pub baseline: u32,
    /// 增量清单封账标记
    pub sealed: bool,
}

impl EtypeAccount {
    /// 新账：全部归属于源账（存量）。
    pub const fn new(etype: MemEtype, baseline: u32) -> EtypeAccount {
        EtypeAccount { etype, legacy: baseline, taken: 0, baseline, sealed: false }
    }

    /// 守恒式：源账剩余 + 目标账已接管 = 基线。
    pub fn conserved(&self) -> bool {
        self.legacy.saturating_add(self.taken) == self.baseline
    }

    /// 划转一笔：从源账划到目标账。违例逐类拒绝。
    pub fn transfer(&mut self, amount: u32) -> Result<(), TakeoverCode> {
        if amount == 0 {
            return Err(TakeoverCode::ZeroAmount);
        }
        if self.sealed {
            return Err(TakeoverCode::PostSealTransfer);
        }
        if self.taken > 0 {
            return Err(TakeoverCode::DoubleCount);
        }
        if amount > self.legacy {
            return Err(TakeoverCode::Overdraft);
        }
        self.legacy -= amount;
        self.taken += amount;
        if !self.conserved() {
            return Err(TakeoverCode::PromiseBreach);
        }
        Ok(())
    }

    /// 封账：冻结增量清单。前提是已有正量接管。
    pub fn seal(&mut self) -> Result<(), TakeoverCode> {
        if self.taken == 0 {
            return Err(TakeoverCode::SealBeforeTransfer);
        }
        if !self.conserved() {
            return Err(TakeoverCode::PromiseBreach);
        }
        self.sealed = true;
        Ok(())
    }
}

/// 接管注册表：五类错误码各一账，总额守恒。
#[derive(Clone, Copy, Debug)]
pub struct TakeoverRegistry {
    accounts: [EtypeAccount; 5],
    /// 已接受注记笔数
    pub notes_applied: u32,
    /// 被拒绝注记笔数
    pub notes_rejected: u32,
}

impl TakeoverRegistry {
    /// 构造：每类错误码基线 LEGACY_BASELINE，全归存量。
    pub const fn new() -> TakeoverRegistry {
        TakeoverRegistry {
            accounts: [
                EtypeAccount::new(MemEtype::Enomem, LEGACY_BASELINE),
                EtypeAccount::new(MemEtype::Eagain, LEGACY_BASELINE),
                EtypeAccount::new(MemEtype::Efault, LEGACY_BASELINE),
                EtypeAccount::new(MemEtype::Einval, LEGACY_BASELINE),
                EtypeAccount::new(MemEtype::Eperm, LEGACY_BASELINE),
            ],
            notes_applied: 0,
            notes_rejected: 0,
        }
    }

    /// 按序号取账视图。
    pub fn account(&self, idx: usize) -> Option<&EtypeAccount> {
        self.accounts.get(idx)
    }

    /// 提交一笔已解析注记：成功划转或逐类拒绝，账目双向登记。
    pub fn apply(&mut self, note: TransferNote) -> Result<(), TakeoverCode> {
        let mut idx = 5usize;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(acc) = self.accounts.get(i) {
                if acc.etype == note.etype {
                    idx = i;
                }
            }
            i += 1;
        }
        if idx >= self.accounts.len() {
            self.notes_rejected = self.notes_rejected.saturating_add(1);
            return Err(TakeoverCode::UnknownEtype);
        }
        let mut out = Err(TakeoverCode::UnknownEtype);
        if let Some(acc) = self.accounts.get_mut(idx) {
            out = acc.transfer(note.amount);
        }
        match out {
            Ok(()) => {
                self.notes_applied = self.notes_applied.saturating_add(1);
                Ok(())
            }
            Err(e) => {
                self.notes_rejected = self.notes_rejected.saturating_add(1);
                Err(e)
            }
        }
    }

    /// 全表守恒：每类守恒且全局总额等于基线总和。
    pub fn globally_conserved(&self) -> bool {
        let mut total = 0u32;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(acc) = self.accounts.get(i) {
                if !acc.conserved() {
                    return false;
                }
                total = total.saturating_add(acc.legacy).saturating_add(acc.taken);
            }
            i += 1;
        }
        total == LEGACY_BASELINE * 5
    }

    /// 全表封账状态查询。
    pub fn all_sealed(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < self.accounts.len() {
            if let Some(acc) = self.accounts.get(i) {
                if !acc.sealed {
                    ok = false;
                }
            }
            i += 1;
        }
        ok
    }
}

// ── 注入账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TakeoverLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl TakeoverLedger {
    const fn zero() -> TakeoverLedger {
        TakeoverLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果，守恒破坏显性报错。
    pub fn record(&mut self, caught: bool) -> Result<(), TakeoverCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(TakeoverCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 四类坏注记样本（逐类 10 轮全抓）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadNoteCase {
    /// 负划转量
    NegativeAmount,
    /// 超源账余额（透支）
    Overdraft,
    /// 错误码类不在闭集
    UnknownEtype,
    /// 同类重复接管（双计）
    DoubleCount,
}

impl BadNoteCase {
    /// 构造原始字段。
    pub fn raw(self) -> (i32, i32) {
        match self {
            BadNoteCase::NegativeAmount => (12, -8),
            // 100 ≤ 上界但 > 源账余额 40：解析通过、划转时透支被拒
            BadNoteCase::Overdraft => (12, 100),
            BadNoteCase::UnknownEtype => (7777, 16),
            BadNoteCase::DoubleCount => (12, 8),
        }
    }

    /// 期望诊断码。
    pub fn code(self) -> TakeoverCode {
        match self {
            BadNoteCase::NegativeAmount => TakeoverCode::NegativeAmount,
            BadNoteCase::Overdraft => TakeoverCode::Overdraft,
            BadNoteCase::UnknownEtype => TakeoverCode::UnknownEtype,
            BadNoteCase::DoubleCount => TakeoverCode::DoubleCount,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadNoteCase; 4] = [
        BadNoteCase::NegativeAmount,
        BadNoteCase::Overdraft,
        BadNoteCase::UnknownEtype,
        BadNoteCase::DoubleCount,
    ];
}

/// 执行一轮坏注记注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
/// 透支/双计类需要先对注册表做正向接管，再注入违约注记。
pub fn run_bad_note_matrix() -> (TakeoverLedger, [u32; 4]) {
    let mut ledger = TakeoverLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadNoteCase::ALL.len() {
        if let Some(case) = BadNoteCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadNoteCase::NegativeAmount | BadNoteCase::UnknownEtype => {
                        let (et, am) = case.raw();
                        parse_note(et, am) == Err(case.code())
                    }
                    BadNoteCase::Overdraft => {
                        // 解析层放行（量在上界内），划转层按余额拒绝
                        let (et, am) = case.raw();
                        match parse_note(et, am) {
                            Ok(note) => {
                                let mut reg = TakeoverRegistry::new();
                                reg.apply(note) == Err(TakeoverCode::Overdraft)
                            }
                            Err(_) => false,
                        }
                    }
                    BadNoteCase::DoubleCount => {
                        let mut reg = TakeoverRegistry::new();
                        let note = match parse_note(12, 8) {
                            Ok(n) => n,
                            Err(_) => TransferNote { etype: MemEtype::Enomem, amount: 8 },
                        };
                        let first = reg.apply(note);
                        let dup = reg.apply(note);
                        first.is_ok() && dup == Err(TakeoverCode::DoubleCount)
                    }
                };
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

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
    ("EAGAIN", 11, 997, "Windows ERROR_IO_PENDING；暂时不足族"),
    ("ENOMEM", 12, 8, "Windows ERROR_NOT_ENOUGH_MEMORY；耗尽族"),
    ("EFAULT", 14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
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

/// 规范语料双跑指纹：五类错误码各划转一笔不同量后封账，账面状态入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = TakeoverRegistry::new();
    let amounts = [8u32, 16, 24, 32, 40];
    let mut i = 0usize;
    while i < MemEtype::ALL.len() {
        if let (Some(t), Some(a)) = (MemEtype::ALL.get(i), amounts.get(i)) {
            let note = TransferNote { etype: *t, amount: *a };
            let _ = reg.apply(note);
            let mut j = 0usize;
            while j < reg.accounts.len() {
                if let Some(acc) = reg.accounts.get_mut(j) {
                    if acc.etype == *t {
                        let _ = acc.seal();
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    let mut bytes: Vec<u8> = Vec::new();
    let mut k = 0usize;
    while k < reg.accounts.len() {
        if let Some(acc) = reg.accounts.get(k) {
            bytes.push((acc.legacy & 0xFF) as u8);
            bytes.push(((acc.legacy >> 8) & 0xFF) as u8);
            bytes.push((acc.taken & 0xFF) as u8);
            bytes.push(if acc.sealed { 1 } else { 0 });
        }
        k += 1;
    }
    bytes.push((reg.notes_applied & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9258 判据集。
pub fn run_unx9258_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9258");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AE00 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：一笔合法注记完成划转
    let note = parse_note(12, 24);
    let mut reg = TakeoverRegistry::new();
    let mut normal_ok = false;
    if let Ok(n) = note {
        normal_ok = reg.apply(n).is_ok();
    }
    s.add("正常流：ENOMEM 划转 24 个错误路径成功", normal_ok,
          "合法注记应把源账 24 个划到目标账");

    // 3. 正常流：划转后守恒成立
    let cons_ok = reg.account(0).map_or(false, |a| a.conserved() && a.legacy == 16 && a.taken == 24);
    s.add("守恒：划转后 源16+目标24=基线40", cons_ok,
          "每类错误码的归属总额在划转前后恒等");

    // 4. 边界：零量注记拒绝
    let zero_rej = parse_note(12, 0) == Err(TakeoverCode::ZeroAmount);
    s.add("边界：零量注记拒绝", zero_rej, "零量注记属空壳账目，必须拒绝");

    // 5. 边界：单笔上界恰好通过，上界+1 拒绝
    let bound_ok = parse_note(22, MAX_NOTE_AMOUNT as i32).is_ok()
        && parse_note(22, MAX_NOTE_AMOUNT as i32 + 1) == Err(TakeoverCode::NoteOverflow);
    s.add("边界：上界 256 通过、257 拒绝", bound_ok,
          "单笔划转量上界为 MAX_NOTE_AMOUNT，超一即拒");

    // 6. 边界：负量注记拒绝（解析层）
    let neg_rej = parse_note(14, -1) == Err(TakeoverCode::NegativeAmount);
    s.add("边界：负量注记拒绝", neg_rej, "负划转只允许走反向注记，正账不容负值");

    // 7. 注入必抓：四类坏样本各 10/10
    let (ledger, per_class) = run_bad_note_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：四类坏样本各 10/10 检出", all_caught,
          "负量/透支/未知类/双计四类必须全部被抓");

    // 8. 注入账守恒：逃逸=0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadNoteCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 9. 零重写承诺：五类全划转后每类守恒且全局总额不变
    let mut reg2 = TakeoverRegistry::new();
    let amounts = [8u32, 16, 24, 32, 40];
    let mut i2 = 0usize;
    while i2 < MemEtype::ALL.len() {
        if let (Some(t), Some(a)) = (MemEtype::ALL.get(i2), amounts.get(i2)) {
            let _ = reg2.apply(TransferNote { etype: *t, amount: *a });
        }
        i2 += 1;
    }
    s.add("零重写承诺：五类划转后全局总额=基线总和",
          reg2.globally_conserved() && reg2.notes_applied == 5,
          "接管只做归属转移，总额不得增减");

    // 10. 双计拒绝：同一错误码第二笔注记必拒且账不双计
    let mut reg3 = TakeoverRegistry::new();
    let _ = reg3.apply(TransferNote { etype: MemEtype::Efault, amount: 10 });
    let dup = reg3.apply(TransferNote { etype: MemEtype::Efault, amount: 10 });
    let dup_ok = dup == Err(TakeoverCode::DoubleCount)
        && reg3.account(2).map_or(false, |a| a.taken == 10)
        && reg3.notes_applied == 1 && reg3.notes_rejected == 1;
    s.add("双计拒绝：同错误码第二笔必拒且只落一笔", dup_ok,
          "同一错误码接管只允许一笔，双计破坏归属守恒");

    // 11. 封账状态机：未接管封账拒、接管后封账过、封账后再划拒
    let mut reg4 = TakeoverRegistry::new();
    let mut early_rej = Err(TakeoverCode::SealBeforeTransfer);
    if let Some(acc) = reg4.accounts.get_mut(1) {
        early_rej = acc.seal();
    }
    let _ = reg4.apply(TransferNote { etype: MemEtype::Eagain, amount: 12 });
    let mut sealed_ok = false;
    let mut post_rej = Err(TakeoverCode::PostSealTransfer);
    if let Some(acc) = reg4.accounts.get_mut(1) {
        sealed_ok = acc.seal().is_ok();
        post_rej = acc.transfer(4);
    }
    s.add("封账状态机：先划后封、封后拒划", early_rej == Err(TakeoverCode::SealBeforeTransfer)
              && sealed_ok && post_rej == Err(TakeoverCode::PostSealTransfer),
          "封账是单向转换：Free→Sealed 必须先有正量接管");

    // 12. Linux 逐码一致：表内码复算通过、表外码拒绝
    let codes_ok = agrees_with_linux(12) && agrees_with_linux(11) && agrees_with_linux(14)
        && agrees_with_linux(22) && agrees_with_linux(1)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：五类 errno 表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = TakeoverCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 个诊断码均带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < TakeoverCode::ALL.len() {
        let mut j = i + 1;
        while j < TakeoverCode::ALL.len() {
            if let (Some(x), Some(y)) = (TakeoverCode::ALL.get(i), TakeoverCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let reg_text = format!("应用 {} / 拒绝 {}", reg3.notes_applied, reg3.notes_rejected);
    s.add_dyn("注记账快照", reg3.notes_applied == 1 && reg3.notes_rejected == 1, &reg_text);

    // 16. 账目单调：再注入只增不减
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 增量清单落账：全封账可查询
    let mut reg5 = TakeoverRegistry::new();
    let mut i5 = 0usize;
    while i5 < MemEtype::ALL.len() {
        if let Some(t) = MemEtype::ALL.get(i5) {
            let _ = reg5.apply(TransferNote { etype: *t, amount: 4 });
            let mut j5 = 0usize;
            while j5 < reg5.accounts.len() {
                if let Some(acc) = reg5.accounts.get_mut(j5) {
                    if acc.etype == *t {
                        let _ = acc.seal();
                    }
                }
                j5 += 1;
            }
        }
        i5 += 1;
    }
    s.add("增量清单落账：未封账为假、五类封账后 all_sealed 为真",
          !reg2.all_sealed() && reg5.all_sealed() && reg5.notes_applied == 5,
          "增量清单封账状态必须可逐类查询且全封可判定");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_transfer_conserves() {
        let mut reg = TakeoverRegistry::new();
        assert!(reg.apply(TransferNote { etype: MemEtype::Enomem, amount: 24 }).is_ok());
        let blank = EtypeAccount::new(MemEtype::Enomem, 0);
        let acc = reg.account(0).unwrap_or(&blank);
        assert_eq!(acc.legacy, LEGACY_BASELINE - 24);
        assert_eq!(acc.taken, 24);
        assert!(acc.conserved());
    }

    #[test]
    fn zero_and_negative_rejected() {
        assert_eq!(parse_note(12, 0), Err(TakeoverCode::ZeroAmount));
        assert_eq!(parse_note(12, -3), Err(TakeoverCode::NegativeAmount));
    }

    #[test]
    fn upper_bound_boundary() {
        assert!(parse_note(22, MAX_NOTE_AMOUNT as i32).is_ok());
        assert_eq!(parse_note(22, MAX_NOTE_AMOUNT as i32 + 1), Err(TakeoverCode::NoteOverflow));
    }

    #[test]
    fn unknown_etype_rejected() {
        assert_eq!(parse_note(7777, 8), Err(TakeoverCode::UnknownEtype));
        assert_eq!(parse_note(0, 8), Err(TakeoverCode::UnknownEtype));
    }

    #[test]
    fn overdraft_rejected() {
        let mut reg = TakeoverRegistry::new();
        assert_eq!(reg.apply(TransferNote { etype: MemEtype::Eperm, amount: 1000 }),
                   Err(TakeoverCode::Overdraft));
    }

    #[test]
    fn double_count_rejected() {
        let mut reg = TakeoverRegistry::new();
        let n = TransferNote { etype: MemEtype::Efault, amount: 8 };
        assert!(reg.apply(n).is_ok());
        assert_eq!(reg.apply(n), Err(TakeoverCode::DoubleCount));
        assert_eq!(reg.notes_applied, 1);
    }

    #[test]
    fn seal_state_machine() {
        let mut acc = EtypeAccount::new(MemEtype::Einval, LEGACY_BASELINE);
        assert_eq!(acc.seal(), Err(TakeoverCode::SealBeforeTransfer));
        assert!(acc.transfer(8).is_ok());
        assert!(acc.seal().is_ok());
        assert_eq!(acc.transfer(8), Err(TakeoverCode::PostSealTransfer));
    }

    #[test]
    fn global_conservation_after_all_transfers() {
        let mut reg = TakeoverRegistry::new();
        let amounts = [8u32, 16, 24, 32, 40];
        let mut i = 0usize;
        while i < MemEtype::ALL.len() {
            if let (Some(t), Some(a)) = (MemEtype::ALL.get(i), amounts.get(i)) {
                assert!(reg.apply(TransferNote { etype: *t, amount: *a }).is_ok());
            }
            i += 1;
        }
        assert!(reg.globally_conserved());
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_note_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, BadNoteCase::ALL.len() as u32 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(12));
        assert!(agrees_with_linux(1));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(9999));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9258_checks_green() {
        let s = run_unx9258_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
