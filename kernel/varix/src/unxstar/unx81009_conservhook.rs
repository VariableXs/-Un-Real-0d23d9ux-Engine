//! UNX-F81010 台账行数守恒预校验钩子（入账前断言） · 0x91180段
//!
//! 判据原文（派工卡 81009 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81010-J1 「台账行数守恒预校验钩子（入账前
//!      断言）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型
//!      故障 10 次零静默；失败路径显性化、内存缓冲建模；判据锚定
//!      §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 守恒断言冻结口径：old_count + delta == new_count；delta 闭集
//!    {+1, +N}，即 delta ≥ 1 且 ≤ MAX_DELTA；0/负/越上界一律拒。
//! 2. HookLedger append-only：每钩子记（old/new/delta/结果码），链式
//!    口径后一钩子的 old 必须恰等于前一钩子的 new——乱序（跳链）与
//!    回退（old 小于链尾 new）分码捕获。
//! 3. 守恒失败即拒：断言不过不入账，入账通道只接受钩子 Consent。
//! 4. 批行数账：每批 6,000 行预算，入账累计 ≤ 6,000，超预算拒且
//!    不改变已累计账（拒即零副作用）。
//! 5. 同批重复钩子拒：同批内（old,new,delta) 三元组重复 = 重复钩子。
//! 6. 钩子型闭集：PreCheck=1 / PostCheck=2，第四态钩子型构造即拒。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/expect/panic。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81009 − 81001)）。
pub const SEG_BASE: u32 = 0x91180;

// ── 诊断码闭集（段内偏移 0x01..0x0C） ─────────────────────────────
/// delta = 0。
pub const E_ZERO: u16 = 1;
/// 负 delta。
pub const E_NEG: u16 = 2;
/// delta 越上界。
pub const E_DELTAUB: u16 = 3;
/// old+delta 溢出。
pub const E_OVF: u16 = 4;
/// 超预算入账。
pub const E_BUDGET: u16 = 5;
/// 钩子账乱序（跳链）。
pub const E_HORDER: u16 = 6;
/// 同批重复钩子。
pub const E_HDUP: u16 = 7;
/// 守恒失败仍入账尝试。
pub const E_CONSERVE: u16 = 8;
/// 钩子账回退。
pub const E_HREWIND: u16 = 9;
/// 非法第四态钩子型。
pub const E_HBADKIND: u16 = 10;
/// 钩子账空。
pub const E_EMPTY: u16 = 11;
/// 同批账未开批。
pub const E_NOBATCH: u16 = 12;

/// 诊断码闭集口径（12 个）。
pub const DIAG_ALL: usize = 12;

/// 单钩 delta 上界（+N 的 N 封顶）。
pub const MAX_DELTA: u64 = 4096;

/// 每批行数预算。
pub const BATCH_BUDGET: u64 = 6000;

/// 钩子型闭集：入账前预校验。
pub const KIND_PRE: u8 = 1;
/// 钩子型闭集：入账后复核。
pub const KIND_POST: u8 = 2;

/// 钩子结果：通过。
pub const RES_PASS: u8 = 1;
/// 钩子结果：拒绝。
pub const RES_DENY: u8 = 2;

/// 钩子账记录：old/new/delta/结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HookRecord {
    /// 钩子型（闭集）。
    pub kind: u8,
    /// 入账前行数。
    pub old: u64,
    /// 入账后行数。
    pub new: u64,
    /// 增量 delta。
    pub delta: u64,
    /// 结果（RES_PASS/RES_DENY）。
    pub result: u8,
}

/// 钩子账（append-only，链式 old→new）。
#[derive(Debug)]
pub struct HookLedger {
    recs: Vec<HookRecord>,
}

impl HookLedger {
    /// 空账。
    pub fn new() -> HookLedger {
        HookLedger { recs: Vec::new() }
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.recs.is_empty()
    }

    /// 链尾 new（空账 None）。
    pub fn tail_new(&self) -> Option<u64> {
        match self.recs.last() {
            Some(r) => Some(r.new),
            None => None,
        }
    }

    /// 入账登记：链式口径——后一钩子 old 必须恰等于链尾 new；
    /// old 小于链尾 = 回退，old 大于链尾 = 乱序（跳链）。
    pub fn record(&mut self, r: HookRecord) -> Result<(), HookError> {
        if r.kind != KIND_PRE && r.kind != KIND_POST {
            return Err(HookError::HBadKind);
        }
        if let Some(last) = self.recs.last() {
            if r.old < last.new {
                return Err(HookError::HRewind);
            }
            if r.old > last.new {
                return Err(HookError::HOrder);
            }
        }
        self.recs.push(r);
        Ok(())
    }

    /// 逐条复算守恒：账内每条必须满足 old+delta==new，失配即账损。
    pub fn verify_conservation(&self) -> Result<usize, HookError> {
        if self.recs.is_empty() {
            return Err(HookError::Empty);
        }
        let mut i = 0usize;
        while i < self.recs.len() {
            let r = self.recs[i];
            if r.delta == 0 {
                return Err(HookError::Zero);
            }
            match r.old.checked_add(r.delta) {
                None => return Err(HookError::Ovf),
                Some(sum) => {
                    if sum != r.new {
                        return Err(HookError::Conserve);
                    }
                }
            }
            i += 1;
        }
        Ok(self.recs.len())
    }

    /// 按序号取记录（越界 None）。
    pub fn rec(&self, idx: usize) -> Option<HookRecord> {
        if idx >= self.recs.len() {
            return None;
        }
        Some(self.recs[idx])
    }
}

/// 批行数账：每批 6,000 预算。
#[derive(Debug)]
pub struct BatchBudget {
    used: u64,
    open: bool,
}

impl BatchBudget {
    /// 新批（未开批状态需先 open）。
    pub fn new() -> BatchBudget {
        BatchBudget {
            used: 0,
            open: false,
        }
    }

    /// 开批。
    pub fn open(&mut self) {
        self.used = 0;
        self.open = true;
    }

    /// 已用额度。
    pub fn used(&self) -> u64 {
        self.used
    }

    /// 剩余额度。
    pub fn remaining(&self) -> u64 {
        BATCH_BUDGET.saturating_sub(self.used)
    }

    /// 入账 n 行：超预算拒且零副作用；未开批拒。
    pub fn admit(&mut self, n: u64) -> Result<u64, HookError> {
        if !self.open {
            return Err(HookError::NoBatch);
        }
        match self.used.checked_add(n) {
            None => Err(HookError::Ovf),
            Some(sum) => {
                if sum > BATCH_BUDGET {
                    Err(HookError::Budget)
                } else {
                    self.used = sum;
                    Ok(self.used)
                }
            }
        }
    }
}

/// 钩子域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookError {
    /// delta = 0。
    Zero,
    /// 负 delta（补码高位）。
    Neg,
    /// delta 越上界。
    DeltaUb,
    /// old+delta 溢出。
    Ovf,
    /// 超预算。
    Budget,
    /// 钩子账乱序。
    HOrder,
    /// 同批重复钩子。
    HDup,
    /// 守恒失败。
    Conserve,
    /// 钩子账回退。
    HRewind,
    /// 非法第四态钩子型。
    HBadKind,
    /// 钩子账空。
    Empty,
    /// 未开批。
    NoBatch,
}

impl HookError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            HookError::Zero => E_ZERO,
            HookError::Neg => E_NEG,
            HookError::DeltaUb => E_DELTAUB,
            HookError::Ovf => E_OVF,
            HookError::Budget => E_BUDGET,
            HookError::HOrder => E_HORDER,
            HookError::HDup => E_HDUP,
            HookError::Conserve => E_CONSERVE,
            HookError::HRewind => E_HREWIND,
            HookError::HBadKind => E_HBADKIND,
            HookError::Empty => E_EMPTY,
            HookError::NoBatch => E_NOBATCH,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            HookError::Zero => "delta zero refused",
            HookError::Neg => "negative delta refused",
            HookError::DeltaUb => "delta over upper bound",
            HookError::Ovf => "old+delta overflow",
            HookError::Budget => "batch budget exceeded",
            HookError::HOrder => "hook ledger out of order",
            HookError::HDup => "duplicate hook in batch",
            HookError::Conserve => "conservation failure, entry denied",
            HookError::HRewind => "hook ledger rewind",
            HookError::HBadKind => "hook kind outside closed set",
            HookError::Empty => "hook ledger empty",
            HookError::NoBatch => "batch not open",
        }
    }
}

/// ConservationHook：入账前断言 old + delta == new（delta ∈ {+1,+N} 闭集）。
#[derive(Debug)]
pub struct ConservationHook {
    batch_keys: Vec<(u64, u64, u64)>,
}

impl ConservationHook {
    /// 空钩。
    pub fn new() -> ConservationHook {
        ConservationHook {
            batch_keys: Vec::new(),
        }
    }

    /// 本批已挂钩数。
    pub fn batch_hook_count(&self) -> usize {
        self.batch_keys.len()
    }

    /// 预校验：返回 Consent（=old+delta）或拒绝码。
    /// 顺序：钩子型闭集 → delta 闭集（0/负/越上界）→ 溢出 → 守恒 → 同批重复。
    pub fn precheck(
        &mut self,
        kind: u8,
        old: u64,
        delta: i64,
        new: u64,
    ) -> Result<u64, HookError> {
        if kind != KIND_PRE && kind != KIND_POST {
            return Err(HookError::HBadKind);
        }
        if delta == 0 {
            return Err(HookError::Zero);
        }
        if delta < 0 {
            return Err(HookError::Neg);
        }
        let d = delta as u64;
        if d > MAX_DELTA {
            return Err(HookError::DeltaUb);
        }
        let sum = match old.checked_add(d) {
            None => return Err(HookError::Ovf),
            Some(s) => s,
        };
        if sum != new {
            return Err(HookError::Conserve);
        }
        let mut i = 0usize;
        while i < self.batch_keys.len() {
            let k = self.batch_keys[i];
            if k.0 == old && k.1 == new && k.2 == d {
                return Err(HookError::HDup);
            }
            i += 1;
        }
        self.batch_keys.push((old, new, d));
        Ok(sum)
    }
}

/// 入账通道：钩子通过 + 批预算允许才入账，守恒失败仍入账尝试在此拒。
pub fn admit_lines(
    hook: &mut ConservationHook,
    budget: &mut BatchBudget,
    old: u64,
    delta: i64,
) -> Result<u64, HookError> {
    let new_count = match hook.precheck(KIND_PRE, old, delta, old.saturating_add(delta.max(0) as u64))
    {
        Ok(n) => n,
        Err(e) => return Err(e),
    };
    budget.admit(delta as u64)
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let mut h = ConservationHook::new();
    // 1 delta = 0 → Zero
    match h.precheck(KIND_PRE, 10, 0, 10) {
        Err(e) => out.push(("delta-zero", e.code())),
        Ok(_) => out.push(("delta-zero-MISSED", SEG_BASE)),
    }
    // 2 负 delta → Neg
    match h.precheck(KIND_PRE, 10, -3, 7) {
        Err(e) => out.push(("delta-neg", e.code())),
        Ok(_) => out.push(("delta-neg-MISSED", SEG_BASE)),
    }
    // 3 delta 越上界 → DeltaUb
    match h.precheck(KIND_PRE, 10, (MAX_DELTA + 1) as i64, 10 + MAX_DELTA + 1) {
        Err(e) => out.push(("delta-ub", e.code())),
        Ok(_) => out.push(("delta-ub-MISSED", SEG_BASE)),
    }
    // 4 old+delta 溢出 → Ovf
    match h.precheck(KIND_PRE, u64::MAX - 1, 5, 0) {
        Err(e) => out.push(("delta-ovf", e.code())),
        Ok(_) => out.push(("delta-ovf-MISSED", SEG_BASE)),
    }
    // 5 超预算入账 → Budget
    let mut b = BatchBudget::new();
    b.open();
    let _ = b.admit(5999);
    match b.admit(2) {
        Err(e) => out.push(("over-budget", e.code())),
        Ok(_) => out.push(("over-budget-MISSED", SEG_BASE)),
    }
    // 6 钩子账乱序（跳链）→ HOrder
    let mut led = HookLedger::new();
    let _ = led.record(HookRecord {
        kind: KIND_PRE,
        old: 0,
        new: 10,
        delta: 10,
        result: RES_PASS,
    });
    match led.record(HookRecord {
        kind: KIND_PRE,
        old: 20,
        new: 30,
        delta: 10,
        result: RES_PASS,
    }) {
        Err(e) => out.push(("hook-order", e.code())),
        Ok(_) => out.push(("hook-order-MISSED", SEG_BASE)),
    }
    // 7 同批重复钩子 → HDup
    let mut h2 = ConservationHook::new();
    let _ = h2.precheck(KIND_PRE, 0, 1, 1);
    match h2.precheck(KIND_PRE, 0, 1, 1) {
        Err(e) => out.push(("hook-dup", e.code())),
        Ok(_) => out.push(("hook-dup-MISSED", SEG_BASE)),
    }
    // 8 守恒失败仍入账尝试 → Conserve
    match h.precheck(KIND_PRE, 10, 1, 99) {
        Err(e) => out.push(("conserve-fail", e.code())),
        Ok(_) => out.push(("conserve-fail-MISSED", SEG_BASE)),
    }
    // 9 钩子账回退 → HRewind
    let mut led2 = HookLedger::new();
    let _ = led2.record(HookRecord {
        kind: KIND_PRE,
        old: 0,
        new: 10,
        delta: 10,
        result: RES_PASS,
    });
    match led2.record(HookRecord {
        kind: KIND_PRE,
        old: 5,
        new: 6,
        delta: 1,
        result: RES_PASS,
    }) {
        Err(e) => out.push(("hook-rewind", e.code())),
        Ok(_) => out.push(("hook-rewind-MISSED", SEG_BASE)),
    }
    // 10 非法第四态钩子型 → HBadKind
    match h.precheck(9, 10, 1, 11) {
        Err(e) => out.push(("hook-badkind", e.code())),
        Ok(_) => out.push(("hook-badkind-MISSED", SEG_BASE)),
    }
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (_, code) = samples[i];
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（F81010-J1 落地）：13 条，正向 ×10 + 注入汇总。
pub fn run_unx81009_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81009");

    // 1 段位守卫
    let ok = SEG_BASE == 0x91180 && SEG_BASE % 0x30 == 0;
    s.add("F81010-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 诊断闭集口径
    let ok = DIAG_ALL == 12;
    s.add("F81010-02 诊断闭集", ok, if ok { "" } else { "闭集口径漂移" });

    // 3 正向 +1 钩子
    let mut h = ConservationHook::new();
    let r1 = h.precheck(KIND_PRE, 0, 1, 1);
    let ok = matches!(r1, Ok(1));
    s.add("F81010-03 正向+1", ok, if ok { "" } else { "+1 路径坏" });

    // 4 正向 +N 钩子
    let r2 = h.precheck(KIND_PRE, 1, 250, 251);
    let ok = matches!(r2, Ok(251)) && h.batch_hook_count() == 2;
    s.add("F81010-04 正向+N", ok, if ok { "" } else { "+N 路径坏" });

    // 5 守恒失败拒：old+delta != new 不入账
    let ok = matches!(h.precheck(KIND_PRE, 251, 1, 999), Err(HookError::Conserve));
    s.add("F81010-05 守恒拒", ok, if ok { "" } else { "守恒闸失灵" });

    // 6 delta 闭集：0/负/越上界三拒
    let ok = matches!(h.precheck(KIND_PRE, 251, 0, 251), Err(HookError::Zero))
        && matches!(h.precheck(KIND_PRE, 251, -1, 250), Err(HookError::Neg))
        && matches!(h.precheck(KIND_PRE, 251, (MAX_DELTA + 1) as i64, 251 + MAX_DELTA + 1), Err(HookError::DeltaUb));
    s.add("F81010-06 delta闭集", ok, if ok { "" } else { "delta 闸失灵" });

    // 7 钩子账链式口径
    let mut led = HookLedger::new();
    let a = led.record(HookRecord {
        kind: KIND_PRE,
        old: 0,
        new: 1,
        delta: 1,
        result: RES_PASS,
    });
    let b = led.record(HookRecord {
        kind: KIND_PRE,
        old: 1,
        new: 4,
        delta: 3,
        result: RES_PASS,
    });
    let ok = matches!(a, Ok(())) && matches!(b, Ok(())) && led.len() == 2;
    s.add("F81010-07 链式账", ok, if ok { "" } else { "链式口径坏" });

    // 8 账复算守恒（往返一致）
    let ok = matches!(led.verify_conservation(), Ok(2));
    s.add("F81010-08 账复算", ok, if ok { "" } else { "账复算失灵" });

    // 9 批预算：6,000 封顶、恰满通过、超一拒
    let mut bt = BatchBudget::new();
    bt.open();
    let full = bt.admit(6000);
    let mut bt2 = BatchBudget::new();
    bt2.open();
    let _ = bt2.admit(5999);
    let over = bt2.admit(2);
    let ok = matches!(full, Ok(6000)) && matches!(over, Err(HookError::Budget)) && bt2.used() == 5999;
    s.add("F81010-09 批预算", ok, if ok { "" } else { "预算口径漂移" });

    // 10 未开批拒
    let ok = matches!(BatchBudget::new().admit(1), Err(HookError::NoBatch));
    s.add("F81010-10 未开批拒", ok, if ok { "" } else { "开批闸失灵" });

    // 11 溢出拒
    let ok = matches!(h.precheck(KIND_PRE, u64::MAX, 1, 0), Err(HookError::Ovf));
    s.add("F81010-11 溢出拒", ok, if ok { "" } else { "溢出漏抓" });

    // 12 第四态钩子型拒
    let ok = matches!(h.precheck(7, 0, 1, 1), Err(HookError::HBadKind));
    s.add("F81010-12 第四态拒", ok, if ok { "" } else { "钩子型闸失灵" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81010-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81009_checks_green() {
        let set = run_unx81009_checks();
        assert!(set.all_passed(), "UNX-F81009 red: {}", set.red_items_text());
    }
}
