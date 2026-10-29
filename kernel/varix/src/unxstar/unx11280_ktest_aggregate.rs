//! UNX-F11280 B04 批 ktest 断言集聚合（0x1fb40 段）
//!
//! 本批 19 条判据（F11274..F11292）要聚进 ktest 生态面，用**一条命令全跑**，
//! 通过率 100% 才算绿。模块把每条判据建模成一条断言：判据号、期望退出码、
//! 实测退出码、超时预算（毫秒）、标志位（跳过/负向用例）。
//! 判据函数按固定序：判据号是否在本批域内 → 超时预算域 → 期望退出码域
//! （0..=127；128 以上表示被信号杀，不属于断言口径）→ 跳过位 → 期望/实测
//! 是否一致。只有全对才返回 0（该条断言通过）。
//! `run_all()` 就是那条「一次命令」：按登记序重算 19 条，累计总数、通过数、
//! 超时预算总和与顺序摘要；`green()` 要求总数恰为 19、通过率 10000 基点。
//! 账本另开一张退出码表（0 一类、1..=127 一类、非法一类）与超时总账：
//! replay 要求表合计等于登记条数、超时总账等于逐条预算之和。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、封册后写账；
//! 期望退出码 200、超时 0 都是**样本**，进规则产码，绝不拒收。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1fb40 独占 0x10）。
pub const SEG_BASE: u32 = 0x1fb40;

/// 条目 id 越界。
pub const KS_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const KS_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const KS_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const KS_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const KS_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const KS_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const KS_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const KS_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const KS_EDOBS: u32 = SEG_BASE + 0x09;
/// 退出码表/超时总账不平。
pub const KS_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：判据号不在本批域内。
pub const AV_RANGE: u32 = 1;
/// 判据码：超时预算非法。
pub const AV_TIMEOUT: u32 = 2;
/// 判据码：期望退出码越界。
pub const AV_EXIT: u32 = 3;
/// 判据码：期望与实测退出码不一致。
pub const AV_MISMATCH: u32 = 4;
/// 判据码：断言被标记跳过。
pub const AV_SKIP: u32 = 5;

/// 本批判据号下界（F11274）。
pub const CRIT_LO: u32 = 11274;
/// 本批判据号上界（F11292）。
pub const CRIT_HI: u32 = 11292;
/// 本批判据条数。
pub const CRIT_COUNT: u32 = 19;
/// 超时预算上限（10 分钟）。
pub const TIMEOUT_MAX: u32 = 600_000;
/// 通过率的满值（基点）。
pub const RATE_FULL: u32 = 10_000;
/// 断言表定容（19 条 + 1 条余量）。
pub const CAP: usize = 20;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

/// 该断言被标记跳过（不计通过）。
pub const F_SKIP: u32 = 1 << 0;
/// 该断言是负向用例（期望非零退出）。
pub const F_NEGATIVE: u32 = 1 << 1;
/// 合法标志位并集。
pub const FLAG_MASK: u32 = F_SKIP | F_NEGATIVE;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_80;

/// 一条断言：判据号 + 期望/实测退出码 + 超时预算 + 标志位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assertion {
    pub id: u32,
    pub crit: u32,
    pub expect_exit: u32,
    pub actual_exit: u32,
    pub timeout_ms: u32,
    pub flags: u32,
}

impl Assertion {
    pub const fn blank() -> Assertion {
        Assertion { id: 0, crit: 0, expect_exit: 0, actual_exit: 0, timeout_ms: 0, flags: 0 }
    }

    pub fn render(&self) -> String {
        format!("A{} F{} want{} got{} t{}ms fl{:x}",
            self.id, self.crit, self.expect_exit, self.actual_exit, self.timeout_ms, self.flags)
    }
}

/// 纯判据：域 → 超时 → 期望退出码 → 跳过 → 实测一致。
pub fn assert_verdict(a: &Assertion) -> u32 {
    if a.crit < CRIT_LO || a.crit > CRIT_HI {
        return AV_RANGE;
    }
    if a.timeout_ms == 0 || a.timeout_ms > TIMEOUT_MAX {
        return AV_TIMEOUT;
    }
    if a.expect_exit > 127 {
        return AV_EXIT;
    }
    if a.flags & F_SKIP != 0 {
        return AV_SKIP;
    }
    if a.actual_exit != a.expect_exit {
        return AV_MISMATCH;
    }
    0
}

/// 退出码表槽：0 一槽、1..=127 一槽、非法一槽。
pub fn exit_slot(expect: u32) -> usize {
    if expect == 0 {
        0
    } else if expect <= 127 {
        1
    } else {
        2
    }
}

/// 一次全跑的报告：总数、通过数、超时预算总和、顺序摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunReport {
    pub total: u32,
    pub passed: u32,
    pub timeout_sum: u32,
    pub digest: u64,
}

impl RunReport {
    pub const fn blank() -> RunReport {
        RunReport { total: 0, passed: 0, timeout_sum: 0, digest: FNV_OFF ^ TAG }
    }

    /// 通过率（基点，四舍五入向下取整；空集记 0）。
    pub fn rate_bps(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        self.passed.wrapping_mul(RATE_FULL) / self.total
    }

    /// 绿：恰好 19 条且通过率满值。
    pub fn green(&self) -> bool {
        self.total == CRIT_COUNT && self.passed == CRIT_COUNT && self.rate_bps() == RATE_FULL
    }
}

/// ktest 聚合账本：断言 + 判据 + 六槽直方 + 退出码表 + 超时总账。
#[derive(Clone, Copy, Debug)]
pub struct SuiteLedger {
    cases: [Assertion; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 6],
    by_exit: [u32; 3],
    timeout_sum: u32,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl SuiteLedger {
    pub const fn new() -> SuiteLedger {
        SuiteLedger {
            cases: [Assertion::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 6],
            by_exit: [0u32; 3],
            timeout_sum: 0,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, a: Assertion) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(KS_EFROZEN));
        }
        if a.id < ID_MIN || a.id > ID_MAX {
            return Err(self.fail(KS_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == a.id {
                    return Err(self.fail(KS_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if a.id <= last.id {
                    return Err(self.fail(KS_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(KS_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = a;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：直方、退出码表、超时总账同步。
    pub fn register_ruled(&mut self, a: Assertion) -> Result<(usize, u32), u32> {
        let idx = self.register(a)?;
        let v = assert_verdict(&a);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if let Some(e) = self.by_exit.get_mut(exit_slot(a.expect_exit)) {
            *e = e.wrapping_add(1);
        }
        self.timeout_sum = self.timeout_sum.wrapping_add(a.timeout_ms);
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(KS_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(KS_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(KS_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn timeout_sum(&self) -> u32 {
        self.timeout_sum
    }
    pub fn passed(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn failed(&self) -> u32 {
        self.len as u32 - self.hist_at(0)
    }
    pub fn rate_bps(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        self.hist_at(0).wrapping_mul(RATE_FULL) / self.len as u32
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 6 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn exit_at(&self, i: usize) -> u32 {
        match self.by_exit.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn exit_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 3 {
            if let Some(v) = self.by_exit.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn crit_span(&self) -> Option<(u32, u32)> {
        if self.len == 0 {
            return None;
        }
        let first = match self.cases.get(0) {
            Some(a) => a.crit,
            None => return None,
        };
        let last = match self.cases.get(self.len - 1) {
            Some(a) => a.crit,
            None => return None,
        };
        Some((first, last))
    }
    pub fn case_at(&self, i: usize) -> Option<Assertion> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// 那条「一次命令」：按登记序重算每条断言，汇总总数/通过/预算/摘要。
    /// 判据从 case 现场重算，不读已落账副本——所以篡改样本立刻反映在通过率上。
    pub fn run_all(&self) -> RunReport {
        let mut r = RunReport::blank();
        let mut k = 0usize;
        while k < self.len {
            if let Some(a) = self.cases.get(k) {
                r.total = r.total.wrapping_add(1);
                r.timeout_sum = r.timeout_sum.wrapping_add(a.timeout_ms);
                r.digest = (r.digest ^ a.crit as u64).wrapping_mul(FNV_PRIME);
                r.digest = (r.digest ^ a.actual_exit as u64).wrapping_mul(FNV_PRIME);
                if assert_verdict(&a) == 0 {
                    r.passed = r.passed.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        r
    }

    /// FNV-1a：断言全字段 + 判据 + 观测位 + 直方 + 退出码表 + 超时总账。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.timeout_sum as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(a) = self.cases.get(k) {
                h = (h ^ a.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.crit as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.expect_exit as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.actual_exit as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.timeout_ms as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ a.flags as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(b) = self.seen.get(k) {
                h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < 6 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        let mut e = 0usize;
        while e < 3 {
            if let Some(v) = self.by_exit.get(e) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            e = e.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算、直方/退出码表/超时总账三平衡。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(KS_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(KS_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(KS_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 || self.exit_sum() != self.len as u32 {
            return Err(KS_EBALANCE);
        }
        let mut tsum = 0u32;
        let mut k = 0usize;
        while k < self.len {
            let a = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(KS_EUNOBS),
            };
            tsum = tsum.wrapping_add(a.timeout_ms);
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(KS_EMISMATCH),
            };
            if assert_verdict(&a) != v {
                return Err(KS_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(KS_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        if tsum != self.timeout_sum {
            return Err(KS_EBALANCE);
        }
        Ok(self.fold())
    }

    /// 复账测试钩子：改实测退出码（让 run_all 的通过率真正掉下来）。
    pub fn retarget(&mut self, i: usize, actual: u32) -> bool {
        match self.cases.get_mut(i) {
            Some(a) => {
                a.actual_exit = actual;
                true
            }
            None => false,
        }
    }

    /// 封册：复账后上锁，二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(KS_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 构造一条断言（id 由调用方补）。
pub fn assertion(crit: u32, expect_exit: u32, actual_exit: u32, timeout_ms: u32, flags: u32) -> Assertion {
    Assertion { id: 1, crit, expect_exit, actual_exit, timeout_ms, flags }
}

/// 十九条规格样本：判据号连续覆盖 F11274..F11292，期望/实测一致。
/// 每第三条是负向用例（期望退出 1），其余期望退出 0。
pub fn spec_of(id: usize) -> (Assertion, u32) {
    if id < 1 || id > CRIT_COUNT as usize {
        return (Assertion::blank(), KS_EID);
    }
    let crit = CRIT_LO.wrapping_add(id as u32 - 1);
    let expect = if crit % 3 == 0 { 1 } else { 0 };
    let flags = if expect == 1 { F_NEGATIVE } else { 0 };
    (assertion(crit, expect, expect, 5000 + id as u32 * 10, flags), 0)
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> SuiteLedger {
    let mut l = SuiteLedger::new();
    let mut i = 1usize;
    while i <= CRIT_COUNT as usize {
        let (mut a, expect) = spec_of(i);
        a.id = i as u32;
        if l.register_ruled(a).is_ok() {
            let _ = l.observe(a.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = SuiteLedger::new();
    let mut hi = assertion(CRIT_LO + 1, 0, 0, 1000, 0);
    hi.id = 2;
    let _ = a.register(hi);
    let bad_a = a.register(assertion(CRIT_LO, 0, 0, 1000, 0)).err() == Some(KS_EORDER);

    let mut b = SuiteLedger::new();
    let one = assertion(CRIT_LO, 0, 0, 1000, 0);
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(KS_EDUP);

    let mut c = SuiteLedger::new();
    let mut wide = assertion(CRIT_LO, 0, 0, 1000, 0);
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(KS_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = assertion(CRIT_LO, 0, 0, 1000, 0);
    late.id = 30;
    let bad_d = d.register(late).err() == Some(KS_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压 19 条聚合与单命令通过率，后四项压拒绝/观测/封册。
pub fn run_unx11280_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11280");

    let layout = SEG_BASE == 0x1fb40
        && KS_EID == SEG_BASE + 0x01
        && KS_EORDER == SEG_BASE + 0x02
        && KS_EDUP == SEG_BASE + 0x03
        && KS_EFULL == SEG_BASE + 0x04
        && KS_EFROZEN == SEG_BASE + 0x05
        && KS_EUNSEEN == SEG_BASE + 0x06
        && KS_EMISMATCH == SEG_BASE + 0x07
        && KS_EUNOBS == SEG_BASE + 0x08
        && KS_EDOBS == SEG_BASE + 0x09
        && KS_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1fb40 段十码连续");

    let led = build_ledger();
    let mut span_ok = led.len() == CRIT_COUNT as usize;
    let mut k = 0usize;
    while k < led.len() {
        if let Some(a) = led.case_at(k) {
            if a.crit != CRIT_LO.wrapping_add(k as u32) {
                span_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S2-19 条判据域连续无缺", span_ok
        && led.crit_span() == Some((CRIT_LO, CRIT_HI))
        && CRIT_HI.wrapping_sub(CRIT_LO).wrapping_add(1) == CRIT_COUNT,
        "F11274..F11292 共 19 条逐条占位，首尾跨度吻合");

    let run = led.run_all();
    s.add("S3-单命令全跑 19 条", run.total == 19
        && run.timeout_sum == 96_900
        && run.digest == build_ledger().run_all().digest,
        "一次重算 19 条，预算合计 96900ms，摘要可复现");

    s.add("S4-通过率满值才算绿", run.passed == 19
        && run.rate_bps() == RATE_FULL
        && run.green()
        && led.rate_bps() == RATE_FULL,
        "19/19 通过，基点 10000，green 成立");

    let mut each = true;
    let mut j = 0usize;
    while j < led.len() {
        if let Some(a) = led.case_at(j) {
            if a.expect_exit != a.actual_exit || led.verdict_at(j) != Some(0) {
                each = false;
            }
        }
        j = j.wrapping_add(1);
    }
    s.add("S5-逐条期望实测一致", each && led.failed() == 0,
        "19 条期望退出码与实测逐条相等，无一条 AV_MISMATCH");

    let exit_gate = assert_verdict(&assertion(CRIT_LO, 127, 127, 1000, 0)) == 0
        && assert_verdict(&assertion(CRIT_LO, 128, 128, 1000, 0)) == AV_EXIT
        && assert_verdict(&assertion(CRIT_LO, 200, 200, 1000, 0)) == AV_EXIT
        && assert_verdict(&assertion(CRIT_LO - 1, 0, 0, 1000, 0)) == AV_RANGE
        && assert_verdict(&assertion(CRIT_HI + 1, 0, 0, 1000, 0)) == AV_RANGE;
    s.add("S6-期望退出码与判据域", exit_gate,
        "127 放行、128/200 判 AV_EXIT，11273/11293 判 AV_RANGE");

    let timeout_gate = assert_verdict(&assertion(CRIT_LO, 0, 0, 0, 0)) == AV_TIMEOUT
        && assert_verdict(&assertion(CRIT_LO, 0, 0, TIMEOUT_MAX + 1, 0)) == AV_TIMEOUT
        && assert_verdict(&assertion(CRIT_LO, 0, 0, TIMEOUT_MAX, 0)) == 0
        && assert_verdict(&assertion(CRIT_LO, 0, 0, 1, 0)) == 0;
    s.add("S7-超时预算域与边界", timeout_gate,
        "0 与 600001 判 AV_TIMEOUT，1..=600000 在域");

    let mut tampered = build_ledger();
    let hit = tampered.retarget(0, 9);
    let bad_run = tampered.run_all();
    s.add("S8-失配跳过与篡改即掉绿", assert_verdict(&assertion(CRIT_LO, 0, 7, 1000, 0)) == AV_MISMATCH
        && assert_verdict(&assertion(CRIT_LO, 0, 7, 1000, F_SKIP)) == AV_SKIP
        && hit
        && bad_run.passed == 18
        && bad_run.rate_bps() < RATE_FULL
        && !bad_run.green()
        && tampered.replay().err() == Some(KS_EMISMATCH),
        "失配 AV_MISMATCH、跳过 AV_SKIP；改一条实测后 18/19 且复账失配");

    s.add("S9-直方退出码表与超时总账", led.hist_at(0) == 19
        && led.hist_sum() == 19
        && led.exit_at(0) == 12
        && led.exit_at(1) == 7
        && led.exit_at(2) == 0
        && led.exit_sum() == 19
        && led.timeout_sum() == 96_900,
        "全绿 19 条；期望退出 0 有 12 条、1 有 7 条、非法 0 条");

    s.add("S10-全观测且判据复算一致", led.replay().is_ok()
        && led.observed() == 19 && led.matched() == 19,
        "十九条逐条复算并观测对齐，三张表互证");

    let mut g = SuiteLedger::new();
    let mut c2 = assertion(CRIT_LO + 1, 0, 0, 1000, 0);
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = assertion(CRIT_LO, 0, 0, 1000, 0);
    c1.id = 1;
    let bad_a = g.register(c1).err() == Some(KS_EORDER);
    let bad_b = g.register(c2).err() == Some(KS_EDUP);
    let mut wide = assertion(CRIT_LO, 0, 0, 1000, 0);
    wide.id = 0;
    let bad_c = g.register(wide).err() == Some(KS_EID);
    let mut full = build_ledger();
    let mut twentieth = assertion(CRIT_HI, 1, 1, 1000, F_NEGATIVE);
    twentieth.id = 20;
    let ok_twenty = full.register(twentieth).is_ok();
    let mut overflow = assertion(CRIT_HI, 0, 0, 1000, 0);
    overflow.id = 21;
    let bad_d = full.register(overflow).err() == Some(KS_EFULL);
    s.add("S11-顺序重复越界定容四闸", bad_a && bad_b && bad_c && bad_d
        && ok_twenty
        && g.accepts() == 1 && g.rejects() == 3 && g.attempts() == 4
        && inject_four() == [true, true, true, true],
        "四闸独立触发；第 20 条在容量内，第 21 条 EFULL");

    let mut ob = SuiteLedger::new();
    let mut one = assertion(CRIT_LO, 0, 0, 1000, 0);
    one.id = 1;
    let _ = ob.register_ruled(one);
    let miss = ob.replay().err() == Some(KS_EUNOBS);
    let unseen = ob.observe(9, 0).err() == Some(KS_EUNSEEN);
    let wrong = ob.observe(1, AV_SKIP) == Ok(false);
    let dob = ob.observe(1, 0).err() == Some(KS_EDOBS);
    s.add("S12-观测面三门", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let root = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = assertion(CRIT_LO, 0, 0, 1000, 0);
    late.id = 30;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", root != 0
        && again == Err(KS_EFROZEN)
        && post == Some(KS_EFROZEN)
        && fz.root() == Some(root)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", root, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11280_batch_of_nineteen() {
        let l = build_ledger();
        assert_eq!(l.len(), 19);
        assert_eq!(l.crit_span(), Some((11274, 11292)));
        assert_eq!(l.passed(), 19);
        assert_eq!(l.rate_bps(), 10_000);
        assert_eq!(l.timeout_sum(), 96_900);
        assert_eq!(l.exit_at(0), 12);
        assert_eq!(l.exit_at(1), 7);
        assert!(l.run_all().green());
        assert!(l.replay().is_ok());
    }

    #[test]
    fn unx11280_rule_gates() {
        assert_eq!(assert_verdict(&assertion(11274, 0, 0, 1000, 0)), 0);
        assert_eq!(assert_verdict(&assertion(11273, 0, 0, 1000, 0)), AV_RANGE);
        assert_eq!(assert_verdict(&assertion(11293, 0, 0, 1000, 0)), AV_RANGE);
        assert_eq!(assert_verdict(&assertion(11274, 0, 0, 0, 0)), AV_TIMEOUT);
        assert_eq!(assert_verdict(&assertion(11274, 0, 0, 600_001, 0)), AV_TIMEOUT);
        assert_eq!(assert_verdict(&assertion(11274, 200, 200, 1000, 0)), AV_EXIT);
        assert_eq!(assert_verdict(&assertion(11274, 1, 2, 1000, 0)), AV_MISMATCH);
        assert_eq!(assert_verdict(&assertion(11274, 1, 2, 1000, F_SKIP)), AV_SKIP);
    }

    #[test]
    fn unx11280_single_red_drops_rate() {
        let mut l = build_ledger();
        assert!(l.retarget(5, 1));
        let r = l.run_all();
        assert_eq!(r.total, 19);
        assert_eq!(r.passed, 18);
        assert_eq!(r.rate_bps(), 9473);
        assert!(!r.green());
        assert_eq!(l.replay().err(), Some(KS_EMISMATCH));
    }

    #[test]
    fn unx11280_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = SuiteLedger::new();
        let a = assertion(11274, 0, 0, 1000, 0);
        assert!(l.register(a).is_ok());
        assert_eq!(l.register(a).err(), Some(KS_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.accepts(), 1);
    }

    #[test]
    fn unx11280_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.freeze(), Err(KS_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11280_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
