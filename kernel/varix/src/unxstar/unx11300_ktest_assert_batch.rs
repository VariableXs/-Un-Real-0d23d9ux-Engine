//! UNX-F11300 B05 批 ktest 断言集聚合（0x20000 段）
//!
//! 本模型把 B05 批 19 条断言收进一张 ktest 生态账。每条断言带编号（1..=19）、
//! 套件号（0..=3）、尝试次数、是否重试通过、是否跳过、是否失败与耗时微秒。
//! 判决规则：首跑通过为 PASS；多次尝试最后一次通过为 RETRY（同样计入已解决）；
//! 跳过为 SKIP（不入通过率分母）；多次尝试仍未通过为 FAIL；编号越界、零次尝试、
//! 零耗时的“空证据通过”一律判 ERROR。批绿判据是分母非零且通过率恰为 1000‰，
//! 同时 FAIL 与 ERROR 为零；SKIP 只登记不参与分母，避免“全跳过”伪装成绿。
//! 结构面五道门：槽号越界、重复登记、乱序登记、空样本与冻结后写入；另设容量门
//! 留在登记路径上，任何拒绝都只增加拒绝计数，不扰动已落格的样本。
//! 票号用 FNV-1a 折叠编号、套件、尝试次数、标志位与耗时，PASS/RETRY 出票，
//! 其余判决票面为零。冻结前重放逐格复算判决与票号，任何一格不符即整账拒收。
//! 账本同时登记语义拒绝数（非 PASS/RETRY 的格数），audit 复算它与分布是否平衡。

use crate::checks::CheckSet;
use crate::alloc::format;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0x20000;

/// 槽号越界。
pub const KA_EID: u32 = SEG_BASE + 0x01;
/// 重复登记。
pub const KA_EDUP: u32 = SEG_BASE + 0x02;
/// 乱序登记。
pub const KA_EORDER: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const KA_EFROZEN: u32 = SEG_BASE + 0x04;
/// 容量已满。
pub const KA_ECAP: u32 = SEG_BASE + 0x05;
/// 空样本。
pub const KA_EEMPTY: u32 = SEG_BASE + 0x06;
/// 断言编号越界。
pub const KA_ECASE: u32 = SEG_BASE + 0x07;
/// 零证据（零尝试或零耗时）。
pub const KA_EZERO: u32 = SEG_BASE + 0x08;
/// 观测不存在的格。
pub const KA_ENOBS: u32 = SEG_BASE + 0x09;
/// 观测与票面不符。
pub const KA_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 账本失衡。
pub const KA_EBAL: u32 = SEG_BASE + 0x0b;
/// 重放票号不符。
pub const KA_ESEAL: u32 = SEG_BASE + 0x0c;

/// 判决：首跑通过。
pub const V_PASS: u8 = 0;
/// 判决：重试后通过。
pub const V_RETRY: u8 = 1;
/// 判决：跳过。
pub const V_SKIP: u8 = 2;
/// 判决：最终未通过。
pub const V_FAIL: u8 = 3;
/// 判决：证据不成立。
pub const V_ERROR: u8 = 4;

/// 本批断言条数。
pub const SAMPLES: usize = 19;
/// 账本容量。
pub const CAPACITY: usize = 19;
/// 断言编号上界。
pub const ID_MAX: u8 = 19;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条断言样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtCase {
    pub id: u8,
    pub suite: u8,
    pub attempts: u8,
    pub retry_ok: bool,
    pub skipped: bool,
    pub failed: bool,
    pub usec: u32,
}

impl KtCase {
    /// 空样本。
    pub const fn blank() -> KtCase {
        KtCase {
            id: 0,
            suite: 0,
            attempts: 0,
            retry_ok: false,
            skipped: false,
            failed: false,
            usec: 0,
        }
    }

    /// 构造断言样本。
    pub const fn make(
        id: u8,
        suite: u8,
        attempts: u8,
        retry_ok: bool,
        skipped: bool,
        failed: bool,
        usec: u32,
    ) -> KtCase {
        KtCase { id, suite, attempts, retry_ok, skipped, failed, usec }
    }

    /// 全零即空样本。
    pub const fn is_blank(&self) -> bool {
        self.id == 0
            && self.suite == 0
            && self.attempts == 0
            && !self.retry_ok
            && !self.skipped
            && !self.failed
            && self.usec == 0
    }

    /// 已解决（计入通过分子）。
    pub const fn is_resolved(&self) -> bool {
        let v = verdict_of(self);
        v == V_PASS || v == V_RETRY
    }
}

/// 观测结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KtObs {
    pub verdict: u8,
    pub resolved: bool,
    pub mark: u64,
}

impl KtObs {
    /// 空结论。
    pub const fn none() -> KtObs {
        KtObs { verdict: V_ERROR, resolved: false, mark: 0 }
    }
}

/// 折叠一个 64 位字。
fn fold_word(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 断言编号是否在登记范围内。
pub fn case_in_range(c: &KtCase) -> bool {
    c.id != 0 && c.id <= ID_MAX
}

/// 样本的判决。
pub fn verdict_of(c: &KtCase) -> u8 {
    if !case_in_range(c) {
        return V_ERROR;
    }
    if c.failed {
        return V_FAIL;
    }
    if c.skipped {
        return V_SKIP;
    }
    if c.attempts == 0 {
        return V_ERROR;
    }
    if c.usec == 0 {
        return V_ERROR;
    }
    if c.attempts > 1 {
        return if c.retry_ok { V_RETRY } else { V_FAIL };
    }
    V_PASS
}

/// 标志位打包（保证折叠覆盖全部离散字段）。
pub fn flags_of(c: &KtCase) -> u64 {
    let mut f = 0u64;
    if c.retry_ok {
        f |= 1;
    }
    if c.skipped {
        f |= 2;
    }
    if c.failed {
        f |= 4;
    }
    f
}

/// 单样本折叠值。
pub fn fold_case(c: &KtCase) -> u64 {
    let mut h = FNV_OFF;
    h = fold_word(h, c.id as u64);
    h = fold_word(h, c.suite as u64);
    h = fold_word(h, c.attempts as u64);
    h = fold_word(h, flags_of(c));
    h = fold_word(h, c.usec as u64);
    h
}

/// 出票：只有 PASS/RETRY 有票号，其余样本结构合法但语义拒绝。
pub fn mark_of(c: &KtCase) -> Result<u64, u32> {
    if !case_in_range(c) {
        return Err(KA_ECASE);
    }
    if !c.skipped && !c.failed && c.attempts == 0 {
        return Err(KA_EZERO);
    }
    if !c.skipped && !c.failed && c.usec == 0 {
        return Err(KA_EZERO);
    }
    match verdict_of(c) {
        V_PASS => Ok(fold_case(c) ^ 0xa5a5),
        V_RETRY => Ok(fold_case(c) ^ 0x5a5a),
        _ => Ok(0),
    }
}

/// 十九格断言账。
pub struct KtLedger {
    cases: Vec<KtCase>,
    verdicts: Vec<u8>,
    marks: Vec<u64>,
    frozen: bool,
    root: u64,
    rejects: u32,
    denials: u32,
}

impl KtLedger {
    /// 空账本。
    pub fn new() -> KtLedger {
        KtLedger {
            cases: Vec::new(),
            verdicts: Vec::new(),
            marks: Vec::new(),
            frozen: false,
            root: 0,
            rejects: 0,
            denials: 0,
        }
    }

    /// 登记一格。结构拒绝返回错误码，语义判决以 Ok(判决) 返回。
    pub fn register(&mut self, slot: u8, case: KtCase) -> Result<u8, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(KA_EFROZEN);
        }
        if slot as usize >= SAMPLES {
            self.rejects += 1;
            return Err(KA_EID);
        }
        if (slot as usize) < self.cases.len() {
            self.rejects += 1;
            return Err(KA_EDUP);
        }
        if slot as usize != self.cases.len() {
            self.rejects += 1;
            return Err(KA_EORDER);
        }
        if self.cases.len() >= CAPACITY {
            self.rejects += 1;
            return Err(KA_ECAP);
        }
        if case.is_blank() {
            self.rejects += 1;
            return Err(KA_EEMPTY);
        }
        let v = verdict_of(&case);
        let mark = match mark_of(&case) {
            Ok(m) => m,
            Err(_) => 0,
        };
        if v != V_PASS && v != V_RETRY {
            self.denials += 1;
        }
        self.cases.push(case);
        self.verdicts.push(v);
        self.marks.push(mark);
        Ok(v)
    }

    /// 已登记格数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 取样本。
    pub fn slot(&self, i: usize) -> Option<KtCase> {
        self.cases.get(i).copied()
    }

    /// 取判决。
    pub fn verdict(&self, i: usize) -> Option<u8> {
        self.verdicts.get(i).copied()
    }

    /// 取票号。
    pub fn mark(&self, i: usize) -> Option<u64> {
        self.marks.get(i).copied()
    }

    /// 某判决的格数。
    pub fn count_verdict(&self, v: u8) -> u32 {
        let mut n = 0u32;
        for x in &self.verdicts {
            if *x == v {
                n += 1;
            }
        }
        n
    }

    /// 已解决格数（PASS + RETRY）。
    pub fn resolved(&self) -> u32 {
        self.count_verdict(V_PASS) + self.count_verdict(V_RETRY)
    }

    /// 通过率（千分比，SKIP 不入分母）。
    pub fn pass_permille(&self) -> u32 {
        let denom = self.count_verdict(V_PASS)
            + self.count_verdict(V_RETRY)
            + self.count_verdict(V_FAIL)
            + self.count_verdict(V_ERROR);
        if denom == 0 {
            return 0;
        }
        (self.resolved() * 1000) / denom
    }

    /// 整批是否绿：满格、零 FAIL、零 ERROR、通过率 1000‰。
    pub fn batch_green(&self) -> bool {
        self.len() == SAMPLES
            && self.count_verdict(V_FAIL) == 0
            && self.count_verdict(V_ERROR) == 0
            && self.pass_permille() == 1000
    }

    /// 语义拒绝数。
    pub fn denials(&self) -> u32 {
        self.denials
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 观测一格并与票面核对。
    pub fn observe(&self, i: usize) -> Result<KtObs, u32> {
        let c = match self.cases.get(i) {
            Some(v) => *v,
            None => return Err(KA_ENOBS),
        };
        let stored = match self.marks.get(i) {
            Some(v) => *v,
            None => return Err(KA_ENOBS),
        };
        let expect = match mark_of(&c) {
            Ok(m) => m,
            Err(_) => 0,
        };
        if stored != expect {
            return Err(KA_EMISMATCH);
        }
        Ok(KtObs { verdict: verdict_of(&c), resolved: expect != 0, mark: stored })
    }

    /// 逐格重算的一致率（千分比）。
    pub fn agree_permille(&self) -> u32 {
        if self.cases.is_empty() {
            return 0;
        }
        let mut ok = 0u32;
        for i in 0..self.cases.len() {
            if verdict_of(&self.cases[i]) == self.verdicts[i]
                && mark_of(&self.cases[i]) == Ok(self.marks[i])
            {
                ok += 1;
            }
        }
        (ok * 1000) / self.cases.len() as u32
    }

    /// 各套件格数。
    pub fn suite_counts(&self) -> [u32; 4] {
        let mut out = [0u32; 4];
        for c in &self.cases {
            if (c.suite as usize) < out.len() {
                out[c.suite as usize] += 1;
            }
        }
        out
    }

    /// 账本折叠。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.cases.len() as u64);
        for i in 0..self.cases.len() {
            h = fold_word(h, fold_case(&self.cases[i]));
            h = fold_word(h, self.verdicts[i] as u64);
            h = fold_word(h, self.marks[i]);
        }
        h = fold_word(h, self.denials as u64);
        h = fold_word(h, self.rejects as u64);
        h
    }

    /// 账本自洽。
    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.verdicts.len() || self.cases.len() != self.marks.len() {
            return Err(KA_EBAL);
        }
        let mut denial = 0u32;
        let mut resolved = 0u32;
        for i in 0..self.cases.len() {
            let v = self.verdicts[i];
            if v != verdict_of(&self.cases[i]) {
                return Err(KA_EBAL);
            }
            if mark_of(&self.cases[i]) != Ok(self.marks[i]) {
                return Err(KA_EBAL);
            }
            if v == V_PASS || v == V_RETRY {
                resolved += 1;
                if self.marks[i] == 0 {
                    return Err(KA_EBAL);
                }
            } else {
                denial += 1;
                if self.marks[i] != 0 {
                    return Err(KA_EBAL);
                }
            }
        }
        if denial != self.denials || resolved != self.resolved() {
            return Err(KA_EBAL);
        }
        Ok(())
    }

    /// 冻结账本。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(KA_EFROZEN);
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：逐格复算判决与票号，返回已解决格数。
    pub fn replay(&self) -> Result<u32, u32> {
        let mut resolved = 0u32;
        for i in 0..self.cases.len() {
            let v = verdict_of(&self.cases[i]);
            if v != self.verdicts[i] {
                return Err(KA_ESEAL);
            }
            if mark_of(&self.cases[i]) != Ok(self.marks[i]) {
                return Err(KA_ESEAL);
            }
            if v == V_PASS || v == V_RETRY {
                resolved += 1;
            }
        }
        if resolved != self.resolved() {
            return Err(KA_ESEAL);
        }
        Ok(resolved)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 已解决断言的（编号, 套件, 票号）表。
pub fn batch_table(l: &KtLedger) -> Vec<(u8, u8, u64)> {
    let mut out = Vec::new();
    for i in 0..l.len() {
        if let (Some(c), Some(m)) = (l.slot(i), l.mark(i)) {
            if m != 0 {
                out.push((c.id, c.suite, m));
            }
        }
    }
    out
}

/// 未解决断言摘要。
pub fn gaps_of(l: &KtLedger) -> Vec<(u8, u8)> {
    let mut out = Vec::new();
    for i in 0..l.len() {
        if let Some(v) = l.verdict(i) {
            if v == V_FAIL || v == V_ERROR || v == V_SKIP {
                if let Some(c) = l.slot(i) {
                    out.push((c.id, v));
                }
            }
        }
    }
    out
}

/// 文本摘要。
pub fn summary(l: &KtLedger) -> String {
    format!(
        "cases={} resolved={} rate={} green={}",
        l.len(),
        l.resolved(),
        l.pass_permille(),
        l.batch_green()
    )
}

/// 构建标准账本：15 首跑通过 + 3 重试通过 + 1 跳过。
pub fn build_ktest() -> KtLedger {
    let mut l = KtLedger::new();
    let _ = l.register(0, KtCase::make(1, 0, 1, false, false, false, 120));
    let _ = l.register(1, KtCase::make(2, 0, 1, false, false, false, 240));
    let _ = l.register(2, KtCase::make(3, 0, 1, false, false, false, 80));
    let _ = l.register(3, KtCase::make(4, 0, 1, false, false, false, 960));
    let _ = l.register(4, KtCase::make(5, 0, 1, false, false, false, 1500));
    let _ = l.register(5, KtCase::make(6, 0, 2, true, false, false, 2200));
    let _ = l.register(6, KtCase::make(7, 0, 1, false, false, false, 410));
    let _ = l.register(7, KtCase::make(8, 0, 1, false, true, false, 0));
    let _ = l.register(8, KtCase::make(9, 1, 2, true, false, false, 1750));
    let _ = l.register(9, KtCase::make(10, 1, 1, false, false, false, 300));
    let _ = l.register(10, KtCase::make(11, 1, 1, false, false, false, 520));
    let _ = l.register(11, KtCase::make(12, 1, 2, true, false, false, 2600));
    let _ = l.register(12, KtCase::make(13, 1, 1, false, false, false, 90));
    let _ = l.register(13, KtCase::make(14, 2, 1, false, false, false, 640));
    let _ = l.register(14, KtCase::make(15, 2, 1, false, false, false, 880));
    let _ = l.register(15, KtCase::make(16, 2, 1, false, false, false, 75));
    let _ = l.register(16, KtCase::make(17, 2, 1, false, false, false, 190));
    let _ = l.register(17, KtCase::make(18, 2, 1, false, false, false, 330));
    let _ = l.register(18, KtCase::make(19, 2, 1, false, false, false, 1250));
    l
}

/// 四路注入：基线、零证据、失败格、冻结锁。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let mut base = build_ktest();
    out[0] = base.audit().is_ok()
        && base.replay() == Ok(18)
        && base.pass_permille() == 1000
        && base.batch_green()
        && base.freeze().is_ok();

    let zero = KtCase::make(20, 0, 1, false, false, false, 0);
    out[1] = verdict_of(&zero) == V_ERROR && mark_of(&zero) == Err(KA_ECASE);

    let flaky = KtCase::make(3, 1, 3, false, false, false, 400);
    out[2] = verdict_of(&flaky) == V_FAIL && !flaky.is_resolved() && mark_of(&flaky) == Ok(0);

    let mut locked = build_ktest();
    let first = locked.freeze().is_ok();
    let root = locked.root();
    let second = locked.freeze() == Err(KA_EFROZEN);
    let write = locked.register(19, KtCase::make(1, 0, 1, false, false, false, 1));
    out[3] = first && root != 0 && second && write == Err(KA_EFROZEN) && locked.rejects() == 1;

    out
}

/// 十三项判据。
pub fn run_unx11300_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11300");
    let l = build_ktest();

    s.add_dyn(
        "KA1 十九条断言齐备",
        l.len() == SAMPLES && !l.is_empty(),
        &format!("cases={} samples={}", l.len(), SAMPLES),
    );

    let pass = l.count_verdict(V_PASS);
    let retry = l.count_verdict(V_RETRY);
    let skip = l.count_verdict(V_SKIP);
    let fail = l.count_verdict(V_FAIL);
    let error = l.count_verdict(V_ERROR);
    s.add_dyn(
        "KA2 判决分布精确",
        pass == 15 && retry == 3 && skip == 1 && fail == 0 && error == 0,
        &format!("p={} r={} s={} f={} e={}", pass, retry, skip, fail, error),
    );

    let mut r = KtLedger::new();
    let ok0 = r.register(0, KtCase::make(1, 0, 1, false, false, false, 50)) == Ok(V_PASS);
    let dup = r.register(0, KtCase::make(2, 0, 1, false, false, false, 60)) == Err(KA_EDUP);
    let order = r.register(3, KtCase::make(4, 0, 1, false, false, false, 70)) == Err(KA_EORDER);
    let id = r.register(19, KtCase::make(1, 0, 1, false, false, false, 80)) == Err(KA_EID);
    let empty = r.register(1, KtCase::blank()) == Err(KA_EEMPTY);
    let froze = r.freeze().is_ok();
    let quiet = r.register(1, KtCase::make(2, 0, 1, false, false, false, 90)) == Err(KA_EFROZEN);
    s.add_dyn(
        "KA3 结构拒绝矩阵五门齐",
        ok0 && dup && order && id && empty && froze && quiet
            && r.rejects() == 5
            && r.len() == 1,
        &format!("rejects={} len={}", r.rejects(), r.len()),
    );

    let high = KtCase::make(20, 0, 1, false, false, false, 100);
    let zero_id = KtCase::make(0, 0, 1, false, false, false, 100);
    s.add_dyn(
        "KA4 断言编号越界拒绝",
        verdict_of(&high) == V_ERROR
            && verdict_of(&zero_id) == V_ERROR
            && mark_of(&high) == Err(KA_ECASE)
            && mark_of(&zero_id) == Err(KA_ECASE),
        &format!("high={:?} low={:?}", mark_of(&high), mark_of(&zero_id)),
    );

    let no_attempt = KtCase::make(4, 0, 0, false, false, false, 300);
    let no_time = KtCase::make(5, 0, 1, false, false, false, 0);
    s.add_dyn(
        "KA5 零证据通过被拒",
        verdict_of(&no_attempt) == V_ERROR
            && verdict_of(&no_time) == V_ERROR
            && mark_of(&no_attempt) == Err(KA_EZERO)
            && mark_of(&no_time) == Err(KA_EZERO),
        &format!("a={:?} t={:?}", mark_of(&no_attempt), mark_of(&no_time)),
    );

    let mut red = KtLedger::new();
    let _ = red.register(0, KtCase::make(1, 0, 1, false, false, false, 10));
    let _ = red.register(1, KtCase::make(2, 0, 2, false, false, false, 20));
    let _ = red.register(2, KtCase::make(3, 0, 1, false, true, false, 0));
    s.add_dyn(
        "KA6 跳过不入分母且失败破绿",
        red.pass_permille() == 500
            && !red.batch_green()
            && red.count_verdict(V_SKIP) == 1
            && red.resolved() == 1,
        &format!("rate={} green={}", red.pass_permille(), red.batch_green()),
    );

    s.add_dyn(
        "KA7 重放逐格一致",
        l.replay() == Ok(18) && l.agree_permille() == 1000,
        &format!("replay={:?} agree={}", l.replay(), l.agree_permille()),
    );

    let mut marks_ok = true;
    let mut i = 0usize;
    while i < l.len() {
        let mut j = i + 1;
        while j < l.len() {
            if let (Some(a), Some(b)) = (l.mark(i), l.mark(j)) {
                if a != 0 && b != 0 && a == b {
                    marks_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add_dyn(
        "KA8 已解决票号两两不同",
        marks_ok && batch_table(&l).len() == 18,
        &format!("tickets={}", batch_table(&l).len()),
    );

    let mut f = build_ktest();
    let first = f.freeze();
    let root = f.root();
    let again = f.freeze() == Err(KA_EFROZEN);
    let locked = f.register(0, KtCase::make(1, 0, 1, false, false, false, 9)) == Err(KA_EFROZEN);
    s.add_dyn(
        "KA9 冻结幂等且写入被锁",
        first.is_ok() && root != 0 && again && locked && f.root() == root,
        &format!("root={:016x}", root),
    );

    let o0 = l.observe(0);
    let o7 = l.observe(7);
    let obs_ok = match o0 {
        Ok(o) => o.verdict == V_PASS && o.resolved && o.mark != 0,
        Err(_) => false,
    };
    let skip_ok = match o7 {
        Ok(o) => o.verdict == V_SKIP && !o.resolved && o.mark == 0,
        Err(_) => false,
    };
    s.add_dyn(
        "KA10 观测与查询一致",
        obs_ok && skip_ok && l.observe(19) == Err(KA_ENOBS),
        &format!("o0={:?} o7={:?}", o0, o7),
    );

    s.add_dyn(
        "KA11 账本平衡复算",
        l.audit().is_ok()
            && l.resolved() == 18
            && l.denials() == 1
            && l.marks.len() == SAMPLES,
        &format!("resolved={} denials={}", l.resolved(), l.denials()),
    );

    let suites = l.suite_counts();
    s.add_dyn(
        "KA12 套件分布与缺口账",
        suites == [8, 5, 6, 0]
            && gaps_of(&l).len() == 1
            && l.slot(18).map(|c| c.id) == Some(19),
        &format!("suites={:?} gaps={}", suites, gaps_of(&l).len()),
    );

    let inj = inject_four();
    s.add_dyn(
        "KA13 四路注入全绿",
        inj == [true; 4],
        &format!("inject={:?}", inj),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_distribution_and_green() {
        let l = build_ktest();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.count_verdict(V_PASS), 15);
        assert_eq!(l.count_verdict(V_RETRY), 3);
        assert_eq!(l.count_verdict(V_SKIP), 1);
        assert_eq!(l.pass_permille(), 1000);
        assert!(l.batch_green());
        assert_eq!(l.denials(), 1);
    }

    #[test]
    fn skip_excluded_from_denominator() {
        let mut l = KtLedger::new();
        assert_eq!(l.register(0, KtCase::make(1, 0, 1, false, false, false, 10)), Ok(V_PASS));
        assert_eq!(l.register(1, KtCase::make(2, 0, 1, false, true, false, 0)), Ok(V_SKIP));
        assert_eq!(l.register(2, KtCase::make(3, 0, 2, false, false, false, 20)), Ok(V_FAIL));
        assert_eq!(l.pass_permille(), 500);
        assert!(!l.batch_green());
        assert_eq!(l.replay(), Ok(1));
    }

    #[test]
    fn evidence_rules() {
        assert_eq!(verdict_of(&KtCase::make(1, 0, 0, false, false, false, 10)), V_ERROR);
        assert_eq!(verdict_of(&KtCase::make(1, 0, 1, false, false, false, 0)), V_ERROR);
        assert_eq!(verdict_of(&KtCase::make(1, 0, 2, true, false, false, 10)), V_RETRY);
        assert_eq!(mark_of(&KtCase::make(1, 0, 2, true, false, false, 10)).is_ok(), true);
        assert_eq!(mark_of(&KtCase::make(0, 0, 1, false, false, false, 10)), Err(KA_ECASE));
    }

    #[test]
    fn refusal_codes_exact() {
        let mut l = KtLedger::new();
        assert_eq!(l.register(0, KtCase::make(1, 0, 1, false, false, false, 10)), Ok(V_PASS));
        assert_eq!(l.register(0, KtCase::make(2, 0, 1, false, false, false, 11)), Err(KA_EDUP));
        assert_eq!(l.register(5, KtCase::make(3, 0, 1, false, false, false, 12)), Err(KA_EORDER));
        assert_eq!(l.register(19, KtCase::make(3, 0, 1, false, false, false, 13)), Err(KA_EID));
        assert_eq!(l.register(1, KtCase::blank()), Err(KA_EEMPTY));
        assert!(l.freeze().is_ok());
        assert_eq!(l.register(1, KtCase::make(3, 0, 1, false, false, false, 14)), Err(KA_EFROZEN));
        assert_eq!(l.rejects(), 5);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn freeze_and_checks_green() {
        let mut l = build_ktest();
        let root = l.freeze();
        assert!(root.is_ok());
        assert_ne!(l.root(), 0);
        assert_eq!(l.freeze(), Err(KA_EFROZEN));
        assert_eq!(l.replay(), Ok(18));
        let s = run_unx11300_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_ktest()).starts_with("cases=19"));
    }
}