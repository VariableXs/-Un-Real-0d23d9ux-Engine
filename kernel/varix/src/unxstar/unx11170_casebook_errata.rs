//! UNX-F11170 域经快照二（检查器口径勘误判例汇编）（0x1f460 段）
//!
//! 纯功能模型：检查器口径勘误汇编三例，每例登记检查器号、旧口径、新口径、
//! 纪律条文号与严重度。终版要求三例齐、新旧口径相异、条文号互异且非零、
//! 检查器互异，并至少落一条严重度为三的纪律条文；锁值后不得再补例。
//!
//! 判例的三道核对面：
//!   一、口径面：旧新口径均在 [0, RULES) 内且相异，禁止空转改口径；
//!   二、条文面：条文号非零、三例互异，重复即拒（一处纪律只编一次）；
//!   三、严重面：严重度在一至三，终版至少一条重条（等级三）。
//!
//! 登记顺序固定为一号、二号、三号；跳号判倒序，重号判重复；
//! 三例齐备后重放方可成功，重放成功才允许锁定，锁定后一切写入返回
//! `ER_EFROZEN`，折叠与重放仍可读。

use crate::checks::CheckSet;

/// 段基址（0x1f460 段）。
pub const SEG_BASE: u32 = 0x1f460;

/// 判例号越界。
pub const ER_ENO: u32 = SEG_BASE + 0x01;

/// 判例号重复。
pub const ER_EDUP: u32 = SEG_BASE + 0x02;

/// 判例号倒序（跳号）。
pub const ER_EORDER: u32 = SEG_BASE + 0x03;

/// 口径越界或新旧同值。
pub const ER_ERULE: u32 = SEG_BASE + 0x04;

/// 条文号为零或重复。
pub const ER_ECLAUSE: u32 = SEG_BASE + 0x05;

/// 严重度越界或终版缺重条。
pub const ER_ESEVER: u32 = SEG_BASE + 0x06;

/// 判例条数不齐即重放。
pub const ER_ECOUNT: u32 = SEG_BASE + 0x07;

/// 终值已锁定。
pub const ER_EFROZEN: u32 = SEG_BASE + 0x08;

/// 检查器号越界。
pub const ER_ECHECK: u32 = SEG_BASE + 0x09;

/// 检查器号重复。
pub const ER_ECHKDUPE: u32 = SEG_BASE + 0x0A;

/// 判例条数。
pub const CASES: usize = 3;

/// 口径编号上限（不含）。
pub const RULES: u8 = 8;

/// 检查器编号上限（不含）。
pub const CHECKERS: u8 = 6;

/// 严重度上限。
pub const MAX_SEVERITY: u8 = 3;

/// 重条严重度。
pub const SEVERITY_HEAVY: u8 = 3;

/// 三例检查器：二号、四号、五号。
pub const CASE_CHECKER: [u8; CASES] = [2, 4, 5];

/// 三例旧口径。
pub const CASE_OLD_RULE: [u8; CASES] = [1, 3, 2];

/// 三例新口径。
pub const CASE_NEW_RULE: [u8; CASES] = [5, 6, 7];

/// 三例纪律条文号。
pub const CASE_CLAUSE: [u16; CASES] = [7, 11, 13];

/// 三例严重度。
pub const CASE_SEVERITY: [u8; CASES] = [1, 2, 3];

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// FNV-1a 单步。
pub fn fnv_step(h: u64, v: u32) -> u64 {
    (h ^ (v as u64)).wrapping_mul(FNV_PRIME)
}

/// 一例口径勘误。
#[derive(Clone, Copy, Debug)]
pub struct Errata {
    /// 判例号（1..3）。
    pub case_no: u8,
    /// 检查器号。
    pub checker: u8,
    /// 旧口径。
    pub old_rule: u8,
    /// 新口径。
    pub new_rule: u8,
    /// 纪律条文号。
    pub clause: u16,
    /// 严重度（1..3）。
    pub severity: u8,
}

impl Errata {
    /// 空例。
    pub const fn blank() -> Self {
        Self {
            case_no: 0,
            checker: 0,
            old_rule: 0,
            new_rule: 0,
            clause: 0,
            severity: 0,
        }
    }

    /// 新旧口径均在域内且相异。
    pub fn rule_ok(&self) -> bool {
        self.old_rule < RULES && self.new_rule < RULES && self.old_rule != self.new_rule
    }

    /// 检查器号在域内。
    pub fn checker_ok(&self) -> bool {
        self.checker < CHECKERS
    }

    /// 条文号非零。
    pub fn clause_ok(&self) -> bool {
        self.clause != 0
    }

    /// 严重度在一至三。
    pub fn severity_ok(&self) -> bool {
        self.severity >= 1 && self.severity <= MAX_SEVERITY
    }

    /// 是否重条。
    pub fn heavy(&self) -> bool {
        self.severity == SEVERITY_HEAVY
    }

    /// 与规则表逐字段相符。
    pub fn agrees(&self) -> bool {
        let i = match self.case_no {
            1 | 2 | 3 => (self.case_no - 1) as usize,
            _ => return false,
        };
        self.checker == CASE_CHECKER[i]
            && self.old_rule == CASE_OLD_RULE[i]
            && self.new_rule == CASE_NEW_RULE[i]
            && self.clause == CASE_CLAUSE[i]
            && self.severity == CASE_SEVERITY[i]
    }
}

/// 规则表：判例号 → (检查器, 旧口径, 新口径, 条文号, 严重度)。
pub const fn rule_case(case_no: u8) -> (u8, u8, u8, u16, u8) {
    let i = (case_no - 1) as usize;
    (
        CASE_CHECKER[i],
        CASE_OLD_RULE[i],
        CASE_NEW_RULE[i],
        CASE_CLAUSE[i],
        CASE_SEVERITY[i],
    )
}

/// 规则校验：判例号、检查器、口径、条文、严重五面，返回首个错误码或 0。
pub fn rule_check(case_no: u8, checker: u8, old_rule: u8, new_rule: u8, clause: u16) -> u32 {
    if case_no == 0 || case_no as usize > CASES {
        return ER_ENO;
    }
    if checker >= CHECKERS {
        return ER_ECHECK;
    }
    if old_rule >= RULES || new_rule >= RULES || old_rule == new_rule {
        return ER_ERULE;
    }
    if clause == 0 {
        return ER_ECLAUSE;
    }
    0
}

/// 按规则表生成一例。
pub fn spec_case(case_no: u8) -> Errata {
    if case_no == 0 || case_no as usize > CASES {
        return Errata::blank();
    }
    let (checker, old_rule, new_rule, clause, severity) = rule_case(case_no);
    Errata {
        case_no,
        checker,
        old_rule,
        new_rule,
        clause,
        severity,
    }
}

/// 判例汇编账本。
#[derive(Clone, Copy, Debug)]
pub struct ErrataBook {
    rows: [Errata; CASES],
    live: [bool; CASES],
    cursor: usize,
    registered: u32,
    rejects: u32,
    frozen: bool,
    sealed: Option<u64>,
}

impl ErrataBook {
    /// 空账本。
    pub const fn new() -> Self {
        Self {
            rows: [Errata::blank(); CASES],
            live: [false; CASES],
            cursor: 0,
            registered: 0,
            rejects: 0,
            frozen: false,
            sealed: None,
        }
    }

    /// 已登记例数。
    pub const fn registered(&self) -> u32 {
        self.registered
    }

    /// 拒绝次数。
    pub const fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否锁定。
    pub const fn frozen(&self) -> bool {
        self.frozen
    }

    /// 登记游标。
    pub const fn cursor(&self) -> usize {
        self.cursor
    }

    /// 锁定值。
    pub const fn sealed(&self) -> Option<u64> {
        self.sealed
    }

    /// 读一例。
    pub fn row(&self, idx: usize) -> Option<&Errata> {
        if idx >= CASES || !self.live[idx] {
            return None;
        }
        self.rows.get(idx)
    }

    /// 记一次拒绝。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 检查器号是否已出现。
    fn checker_taken(&self, checker: u8, skip: usize) -> bool {
        let mut i = 0usize;
        while i < CASES {
            if i != skip && self.live[i] && self.rows[i].checker == checker {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 条文号是否已出现。
    fn clause_taken(&self, clause: u16, skip: usize) -> bool {
        let mut i = 0usize;
        while i < CASES {
            if i != skip && self.live[i] && self.rows[i].clause == clause {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 登记一例：六面把关（冻结、号域、重复、顺序、口径、检查器、条文、严重度）。
    pub fn register(
        &mut self,
        case_no: u8,
        checker: u8,
        old_rule: u8,
        new_rule: u8,
        clause: u16,
        severity: u8,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(self.fail(ER_EFROZEN));
        }
        if case_no == 0 || case_no as usize > CASES {
            return Err(self.fail(ER_ENO));
        }
        let idx = (case_no - 1) as usize;
        if self.live[idx] {
            return Err(self.fail(ER_EDUP));
        }
        if case_no as usize != self.cursor + 1 {
            return Err(self.fail(ER_EORDER));
        }
        let code = rule_check(case_no, checker, old_rule, new_rule, clause);
        if code != 0 {
            return Err(self.fail(code));
        }
        if self.checker_taken(checker, idx) {
            return Err(self.fail(ER_ECHKDUPE));
        }
        if self.clause_taken(clause, idx) {
            return Err(self.fail(ER_ECLAUSE));
        }
        if severity < 1 || severity > MAX_SEVERITY {
            return Err(self.fail(ER_ESEVER));
        }
        self.rows[idx] = Errata {
            case_no,
            checker,
            old_rule,
            new_rule,
            clause,
            severity,
        };
        self.live[idx] = true;
        self.cursor = self.cursor.wrapping_add(1);
        self.registered = self.registered.wrapping_add(1);
        Ok(())
    }

    /// 登记规则表原例。
    pub fn register_spec(&mut self, case_no: u8) -> Result<(), u32> {
        let e = spec_case(case_no);
        self.register(e.case_no, e.checker, e.old_rule, e.new_rule, e.clause, e.severity)
    }
}

/// 不变量、重放与锁定。
impl ErrataBook {
    /// 三例齐备。
    pub fn complete(&self) -> bool {
        self.cursor == CASES && self.registered as usize == CASES
    }

    /// 条文号三例互异。
    pub fn clauses_unique(&self) -> bool {
        if !self.complete() {
            return false;
        }
        let mut i = 0usize;
        while i < CASES {
            if self.clause_taken(self.rows[i].clause, i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 检查器号三例互异。
    pub fn checkers_unique(&self) -> bool {
        if !self.complete() {
            return false;
        }
        let mut i = 0usize;
        while i < CASES {
            if self.checker_taken(self.rows[i].checker, i) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 至少一条重条。
    pub fn heavy_present(&self) -> bool {
        let mut i = 0usize;
        while i < CASES {
            if self.live[i] && self.rows[i].heavy() {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 三例均与规则表逐字段相符。
    pub fn agrees_rule(&self) -> bool {
        if !self.complete() {
            return false;
        }
        let mut i = 0usize;
        while i < CASES {
            if !self.rows[i].agrees() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 折叠全账为单一指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < CASES {
            let e = &self.rows[i];
            h = fnv_step(h, e.case_no as u32);
            h = fnv_step(h, e.checker as u32);
            h = fnv_step(h, e.old_rule as u32);
            h = fnv_step(h, e.new_rule as u32);
            h = fnv_step(h, e.clause as u32);
            h = fnv_step(h, e.severity as u32);
            h = fnv_step(h, self.live[i] as u32);
            i += 1;
        }
        h = fnv_step(h, self.registered);
        h
    }

    /// 终重放：条数、互异、规则、重条四道闸。
    pub fn replay(&self) -> Result<u64, u32> {
        if !self.complete() {
            return Err(ER_ECOUNT);
        }
        if !self.heavy_present() {
            return Err(ER_ESEVER);
        }
        if !self.agrees_rule() {
            return Err(ER_ERULE);
        }
        if !self.clauses_unique() {
            return Err(ER_ECLAUSE);
        }
        if !self.checkers_unique() {
            return Err(ER_ECHKDUPE);
        }
        Ok(self.fold())
    }

    /// 锁定：先重放后落锁，只成功一次。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(self.fail(ER_EFROZEN));
        }
        let h = match self.replay() {
            Ok(h) => h,
            Err(e) => return Err(self.fail(e)),
        };
        self.frozen = true;
        self.sealed = Some(h);
        Ok(h)
    }
}

/// 搭建三例齐备的判例汇编。
pub fn build_casebook() -> ErrataBook {
    let mut b = ErrataBook::new();
    let mut i = 1u8;
    while i <= CASES as u8 {
        let _ = b.register_spec(i);
        i += 1;
    }
    b
}

/// 四向注入：越界、重复、跳号、错口径，各应被拒。
pub fn inject_four() -> [bool; 4] {
    let mut a = build_casebook();
    let i0 = a.register_spec(4) == Err(ER_ENO);
    let i1 = a.register_spec(1) == Err(ER_EDUP);
    let mut b = ErrataBook::new();
    let i2 = b.register_spec(2) == Err(ER_EORDER);
    let mut c = ErrataBook::new();
    let i3 = c.register(1, CASE_CHECKER[0], 4, 4, CASE_CLAUSE[0], 1) == Err(ER_ERULE);
    [i0, i1, i2, i3]
}

/// 本模块十三道判据。
pub fn run_unx11170_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11170 域经快照二");
    let b = build_casebook();

    s.add(
        "E1 三例齐备、零拒绝",
        b.complete() && b.registered() == CASES as u32 && b.rejects() == 0,
        "3/3",
    );

    let (c1, o1, n1, k1, v1) = rule_case(1);
    let (c3, o3, n3, k3, v3) = rule_case(3);
    s.add(
        "E2 规则表一号与三号字段对平",
        (c1, o1, n1, k1, v1) == (2, 1, 5, 7, 1) && (c3, o3, n3, k3, v3) == (5, 2, 7, 13, 3),
        "2/1/5/7/1 · 5/2/7/13/3",
    );

    s.add(
        "E3 三例新旧口径均相异且在域内",
        b.agrees_rule()
            && CASE_OLD_RULE.iter().zip(CASE_NEW_RULE.iter()).all(|(o, n)| o != n && *o < RULES && *n < RULES),
        "口径 3/3",
    );

    s.add(
        "E4 三条纪律条文互异且非零",
        b.clauses_unique() && CASE_CLAUSE[0] != 0 && CASE_CLAUSE[1] != 0 && CASE_CLAUSE[2] != 0,
        "条文 3 异",
    );

    s.add(
        "E5 检查器号互异、各在域内",
        b.checkers_unique()
            && CASE_CHECKER[0] < CHECKERS
            && CASE_CHECKER[1] < CHECKERS
            && CASE_CHECKER[2] < CHECKERS,
        "检查器 3 异",
    );

    s.add(
        "E6 至少一条严重度三的纪律条文",
        b.heavy_present() && CASE_SEVERITY[2] == SEVERITY_HEAVY,
        "重条 1",
    );

    s.add(
        "E7 三例逐字段与规则表相符",
        b.agrees_rule() && b.row(0).map(|e| e.checker) == Some(2) && b.row(2).map(|e| e.clause) == Some(13),
        "相符 3/3",
    );

    let mut ord = ErrataBook::new();
    let order = ord.register_spec(2) == Err(ER_EORDER)
        && ord.register_spec(1).is_ok()
        && ord.register_spec(1) == Err(ER_EDUP)
        && ord.register_spec(4) == Err(ER_ENO);
    s.add(
        "E8 跳号 EORDER、重号 EDUP、越界 ENO",
        order && ord.registered() == 1 && ord.rejects() == 3,
        "三拒",
    );

    let mut rule = ErrataBook::new();
    let same = rule.register(1, 2, 3, 3, 7, 1) == Err(ER_ERULE);
    let out = rule.register(1, 2, 1, 9, 7, 1) == Err(ER_ERULE);
    let chk = rule.register(1, 9, 1, 5, 7, 1) == Err(ER_ECHECK);
    let cla = rule.register(1, 2, 1, 5, 0, 1) == Err(ER_ECLAUSE);
    s.add(
        "E9 同口径/越界口径 ER_RULE、检查器越界、条文零",
        same && out && chk && cla,
        "四拒",
    );

    let mut dup = ErrataBook::new();
    let _ = dup.register_spec(1);
    let dupe_chk =
        dup.register(2, CASE_CHECKER[0], CASE_OLD_RULE[1], CASE_NEW_RULE[1], CASE_CLAUSE[1], 2)
            == Err(ER_ECHKDUPE);
    let mut dup2 = ErrataBook::new();
    let _ = dup2.register_spec(1);
    let dupe_cla =
        dup2.register(2, CASE_CHECKER[1], CASE_OLD_RULE[1], CASE_NEW_RULE[1], CASE_CLAUSE[0], 2)
            == Err(ER_ECLAUSE);
    s.add(
        "E10 检查器重号 ECHKDUPE、条文重号 ECLAUSE",
        dupe_chk && dupe_cla,
        "双重复拒",
    );

    let mut sev = ErrataBook::new();
    let bad = sev.register(1, 2, 1, 5, 7, 0) == Err(ER_ESEVER)
        && sev.register(1, 2, 1, 5, 7, 4) == Err(ER_ESEVER);
    let mut light = ErrataBook::new();
    let _ = light.register(1, 2, 1, 5, 9, 1);
    let _ = light.register(2, 4, 3, 6, 11, 2);
    let _ = light.register(3, 5, 2, 7, 13, 2);
    s.add(
        "E11 严重度越界拒收、无重条则重放 ESEVER",
        bad && light.replay() == Err(ER_ESEVER),
        "ESEVER ×3",
    );

    s.add(
        "E12 重放与折叠同指纹、三例局缺即 ECOUNT",
        b.replay() == Ok(b.fold())
            && {
                let mut half = ErrataBook::new();
                let _ = half.register_spec(1);
                half.replay() == Err(ER_ECOUNT)
            },
        "一致 + 缺例",
    );

    let inj = inject_four();
    let mut all = true;
    let mut z = 0usize;
    while z < 4 {
        if !inj[z] {
            all = false;
        }
        z = z.wrapping_add(1);
    }
    let mut fz = build_casebook();
    let once = fz.seal().is_ok() && fz.seal() == Err(ER_EFROZEN) && fz.sealed().is_some();
    s.add(
        "E13 四向注入全真、判例汇编锁定一次性",
        all && once,
        "注入 4/4 + 锁定",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn casebook_builds_clean() {
        let b = build_casebook();
        assert_eq!(b.registered(), 3);
        assert_eq!(b.rejects(), 0);
        assert!(b.complete());
        assert!(b.agrees_rule());
        assert_eq!(b.replay(), Ok(b.fold()));
    }

    #[test]
    fn rule_table_edges() {
        assert_eq!(rule_case(1), (2, 1, 5, 7, 1));
        assert_eq!(rule_case(2), (4, 3, 6, 11, 2));
        assert_eq!(rule_case(3), (5, 2, 7, 13, 3));
        assert_eq!(rule_check(1, 2, 1, 5, 7), 0);
        assert_eq!(rule_check(0, 2, 1, 5, 7), ER_ENO);
        assert_eq!(rule_check(1, 2, 5, 5, 7), ER_ERULE);
        assert_eq!(rule_check(1, 2, 1, 5, 0), ER_ECLAUSE);
        assert_eq!(rule_check(1, 6, 1, 5, 7), ER_ECHECK);
    }

    #[test]
    fn no_heavy_is_refused() {
        let mut b = ErrataBook::new();
        assert!(b.register(1, 2, 1, 5, 9, 1).is_ok());
        assert!(b.register(2, 4, 3, 6, 11, 2).is_ok());
        assert!(b.register(3, 5, 2, 7, 13, 2).is_ok());
        assert_eq!(b.replay(), Err(ER_ESEVER));
        assert_eq!(b.seal(), Err(ER_ESEVER));
    }

    #[test]
    fn refusal_gates() {
        let mut b = ErrataBook::new();
        assert_eq!(b.register_spec(3), Err(ER_EORDER));
        assert!(b.register_spec(1).is_ok());
        assert_eq!(b.register_spec(1), Err(ER_EDUP));
        assert_eq!(
            b.register(2, CASE_CHECKER[0], 3, 6, 11, 2),
            Err(ER_ECHKDUPE)
        );
        assert_eq!(
            b.register(2, CASE_CHECKER[1], 3, 6, CASE_CLAUSE[0], 2),
            Err(ER_ECLAUSE)
        );
        assert_eq!(b.register(2, CASE_CHECKER[1], 3, 6, 11, 0), Err(ER_ESEVER));
        assert!(b.register_spec(2).is_ok());
        assert!(b.register_spec(3).is_ok());
        assert_eq!(b.replay(), Ok(b.fold()));
        assert!(b.seal().is_ok());
        assert_eq!(b.seal(), Err(ER_EFROZEN));
        assert_eq!(b.rejects(), 6);
    }

    #[test]
    fn inject_all_fire() {
        let inj = inject_four();
        let mut i = 0usize;
        while i < 4 {
            assert!(inj[i]);
            i += 1;
        }
        assert!(spec_case(0).case_no == 0);
        assert_eq!(spec_case(3).severity, 3);
        assert!(spec_case(3).heavy());
    }

    #[test]
    fn checks_green() {
        let s = run_unx11170_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
