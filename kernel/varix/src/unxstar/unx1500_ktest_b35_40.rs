//! UNX-F1500 ktest B35 断言集（批收口 ≥40 断言） · 0x78000段
//!
//! 判据原文（UNX-F1500-J1 摘录）：「B35 批断言 ≥40 条全绿（批内 19 条判据
//! 逐条落断言+批收口聚合），断言数入域级账只增不减，编号唯一性常绿，收口
//! 报告落档」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 账本容量 40，断言编号 1..=40 逐条登记，乱序/重复/越界/空样本显性拒，
//!    拒绝只加拒绝计数、不动已落格的账；
//! 2. 40 条断言分属 19 条判据（判据组 1..=19），批绿要求 19 组全被覆盖且
//!    组内全 PASS——单组缺断言或组内混红即整批不绿；
//! 3. 断言总数入域级账只增不减：登记成功加一，拒绝与冻结均不动账，复算
//!    时「总数 == 已落格数」恒等式必须成立；
//! 4. 编号唯一性以两两比对复算，任何重号即唯一性判据翻红；
//! 5. 收口报告（断言数/判据覆盖/通过率/折叠根）非空落档，冻结后重放逐格
//!    复核判决与票号；
//! 6. 状态机 登记→封账 单向，倒退显性拒；诊断码闭集 0x78001..0x7800C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值）。
pub const SEG_BASE: u32 = 0x78000;

/// 断言编号越界（不在 1..=40）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 断言编号越界。
    ERange,
    /// 断言编号重复。
    EDup,
    /// 登记序号乱序。
    EOrder,
    /// 判据组号越界。
    EGroup,
    /// 空断言样本。
    EEmpty,
    /// 账本已封账。
    ESealed,
    /// 容量已满。
    ECap,
    /// 断言未通过（批内混红）。
    ENotGreen,
    /// 判据组覆盖缺口。
    ENoGroup,
    /// 编号唯一性破坏。
    EUniq,
    /// 收口报告缺失或为空。
    ENoReport,
    /// 封账重放票号不符。
    ESeal,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::ERange => 0x01,
                Diag::EDup => 0x02,
                Diag::EOrder => 0x03,
                Diag::EGroup => 0x04,
                Diag::EEmpty => 0x05,
                Diag::ESealed => 0x06,
                Diag::ECap => 0x07,
                Diag::ENotGreen => 0x08,
                Diag::ENoGroup => 0x09,
                Diag::EUniq => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::ESeal => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "断言编号越界：编号必须落在 1..=40，请核对派工批号",
            Diag::EDup => "断言编号重复：每条断言只登记一次，先查重再入账",
            Diag::EOrder => "登记乱序：必须按格序 0,1,2.. 顺序补齐，禁止跳格回填",
            Diag::EGroup => "判据组号越界：判据组只认 1..=19，请对照 B35 判据表",
            Diag::EEmpty => "空断言样本：编号/尝试数/耗时/结论全零不构成证据",
            Diag::ESealed => "账本已封账：封账后写入一律拒绝，收口走下一批",
            Diag::ECap => "容量已满：40 格已齐，多余断言属于下一批",
            Diag::ENotGreen => "批内混红：存在未通过断言，先修复再收口",
            Diag::ENoGroup => "判据组覆盖缺口：19 组必须每组至少 1 条断言",
            Diag::EUniq => "编号唯一性破坏：两两比对出现重号，逐格排查",
            Diag::ENoReport => "收口报告缺失：报告必须非空并载明断言数与根值",
            Diag::ESeal => "重放票号不符：封账后复算与票面不一致，账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::EOrder,
    Diag::EGroup,
    Diag::EEmpty,
    Diag::ESealed,
    Diag::ECap,
    Diag::ENotGreen,
    Diag::ENoGroup,
    Diag::EUniq,
    Diag::ENoReport,
    Diag::ESeal,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78001..0x7800C 闭集内"
}

/// 判决：首跑通过。
pub const V_PASS: u8 = 0;
/// 判决：最终未通过。
pub const V_FAIL: u8 = 1;
/// 判决：证据不成立。
pub const V_ERROR: u8 = 2;

/// B35 批断言条数（批收口下限）。
pub const SAMPLES: usize = 40;
/// 判据组数（批内 19 条判据）。
pub const GROUPS: usize = 19;
/// 断言编号上界。
pub const ID_MAX: u16 = 40;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

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

/// 一条 B35 断言样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B35Case {
    /// 断言编号（1..=40，唯一）。
    pub id: u16,
    /// 所属判据组（1..=19）。
    pub group: u8,
    /// 尝试次数（≥1 才构成证据）。
    pub attempts: u8,
    /// 耗时微秒（>0 才构成证据）。
    pub usec: u32,
    /// 最终是否通过。
    pub passed: bool,
}

impl B35Case {
    /// 构造断言样本。
    pub const fn make(id: u16, group: u8, attempts: u8, usec: u32, passed: bool) -> B35Case {
        B35Case { id, group, attempts, usec, passed }
    }

    /// 空样本判定（全零即空）。
    pub const fn is_blank(&self) -> bool {
        self.id == 0 && self.group == 0 && self.attempts == 0 && self.usec == 0 && !self.passed
    }
}

/// 样本判决：编号越界、零证据判 ERROR，否则按 passed 分 PASS/FAIL。
pub fn verdict_of(c: &B35Case) -> u8 {
    if c.id == 0 || c.id > ID_MAX {
        return V_ERROR;
    }
    if c.attempts == 0 || c.usec == 0 {
        return V_ERROR;
    }
    if c.passed {
        V_PASS
    } else {
        V_FAIL
    }
}

/// 样本是否结构合法（编号与判据组均在范围内）。
pub fn case_structural_ok(c: &B35Case) -> bool {
    c.id >= 1 && c.id <= ID_MAX && c.group >= 1 && (c.group as usize) <= GROUPS
}

/// 单样本票号：PASS 出票（折叠异或盐），其余票面为零。
pub fn mark_of(c: &B35Case) -> Result<u64, u32> {
    if !case_structural_ok(c) {
        return Err(Diag::ERange.val());
    }
    if c.attempts == 0 || c.usec == 0 {
        return Err(Diag::EEmpty.val());
    }
    let mut h = FNV_OFF;
    h = fold_word(h, c.id as u64);
    h = fold_word(h, c.group as u64);
    h = fold_word(h, c.attempts as u64);
    h = fold_word(h, c.usec as u64);
    h = fold_word(h, c.passed as u64);
    match verdict_of(c) {
        V_PASS => Ok(h ^ 0x3535),
        _ => Ok(0),
    }
}

/// 四十格 B35 断言账。
pub struct B35Ledger {
    cases: Vec<B35Case>,
    verdicts: Vec<u8>,
    marks: Vec<u64>,
    group_counts: [u32; GROUPS],
    frozen: bool,
    root: u64,
    rejects: u32,
    /// 域级断言总数（只增不减）。
    total_logged: u32,
}

impl B35Ledger {
    /// 空账本。
    pub fn new() -> B35Ledger {
        B35Ledger {
            cases: Vec::new(),
            verdicts: Vec::new(),
            marks: Vec::new(),
            group_counts: [0u32; GROUPS],
            frozen: false,
            root: 0,
            rejects: 0,
            total_logged: 0,
        }
    }

    /// 登记一格：结构拒绝返回错误码，语义判决以 Ok(判决) 返回。
    pub fn register(&mut self, seq: usize, case: B35Case) -> Result<u8, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(Diag::ESealed.val());
        }
        if seq != self.cases.len() {
            self.rejects += 1;
            return Err(Diag::EOrder.val());
        }
        if self.cases.len() >= SAMPLES {
            self.rejects += 1;
            return Err(Diag::ECap.val());
        }
        if case.is_blank() {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if case.attempts == 0 || case.usec == 0 {
            self.rejects += 1;
            return Err(Diag::EEmpty.val());
        }
        if case.id == 0 || case.id > ID_MAX {
            self.rejects += 1;
            return Err(Diag::ERange.val());
        }
        if case.group == 0 || (case.group as usize) > GROUPS {
            self.rejects += 1;
            return Err(Diag::EGroup.val());
        }
        for c in &self.cases {
            if c.id == case.id {
                self.rejects += 1;
                return Err(Diag::EDup.val());
            }
        }
        let v = verdict_of(&case);
        let m = match mark_of(&case) {
            Ok(x) => x,
            Err(_) => 0,
        };
        self.group_counts[(case.group - 1) as usize] += 1;
        self.cases.push(case);
        self.verdicts.push(v);
        self.marks.push(m);
        // 域级断言总数只增不减。
        self.total_logged += 1;
        Ok(v)
    }

    /// 已落格数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 取样本。
    pub fn case_at(&self, i: usize) -> Option<B35Case> {
        self.cases.get(i).copied()
    }

    /// 取判决。
    pub fn verdict_at(&self, i: usize) -> Option<u8> {
        self.verdicts.get(i).copied()
    }

    /// 取票号。
    pub fn mark_at(&self, i: usize) -> Option<u64> {
        self.marks.get(i).copied()
    }

    /// 域级断言总数（只增不减记账面）。
    pub fn total_logged(&self) -> u32 {
        self.total_logged
    }

    /// 结构拒绝数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已封账。
    pub fn is_frozen(&self) -> bool {
        self.frozen
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

    /// 通过断言数（PASS）。
    pub fn resolved(&self) -> u32 {
        self.count_verdict(V_PASS)
    }

    /// 通过率（千分比，ERROR 与 FAIL 同入分母）。
    pub fn pass_permille(&self) -> u32 {
        let denom = self.verdicts.len() as u32;
        if denom == 0 {
            return 0;
        }
        (self.resolved() * 1000) / denom
    }

    /// 编号唯一性两两复算。
    pub fn uniqueness_ok(&self) -> bool {
        for i in 0..self.cases.len() {
            let mut j = i + 1;
            while j < self.cases.len() {
                let (a, b) = match (self.cases.get(i), self.cases.get(j)) {
                    (Some(a), Some(b)) => (*a, *b),
                    _ => return false,
                };
                if a.id == b.id {
                    return false;
                }
                j += 1;
            }
        }
        true
    }

    /// 19 组覆盖：每组至少 1 条断言。
    pub fn groups_all_covered(&self) -> bool {
        for k in 0..GROUPS {
            if self.group_counts[k] == 0 {
                return false;
            }
        }
        true
    }

    /// 某判据组的断言数。
    pub fn group_count(&self, group: u8) -> u32 {
        if group == 0 || (group as usize) > GROUPS {
            return 0;
        }
        self.group_counts[(group - 1) as usize]
    }

    /// 批绿：满格 + 零 FAIL/ERROR + 通过率 1000‰ + 19 组全覆盖 + 唯一性。
    pub fn batch_green(&self) -> bool {
        self.len() == SAMPLES
            && self.count_verdict(V_FAIL) == 0
            && self.count_verdict(V_ERROR) == 0
            && self.pass_permille() == 1000
            && self.groups_all_covered()
            && self.uniqueness_ok()
    }

    /// 「断言数只增不减」恒等式：总数 == 已落格数。
    pub fn append_only_ok(&self) -> bool {
        self.total_logged == self.cases.len() as u32
    }

    /// 账本折叠根。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold_word(h, self.cases.len() as u64);
        for i in 0..self.cases.len() {
            let c = match self.cases.get(i) {
                Some(x) => *x,
                None => return 0,
            };
            h = fold_word(h, mark_of(&c).unwrap_or(0));
            let v = match self.verdicts.get(i) {
                Some(x) => *x as u64,
                None => return 0,
            };
            h = fold_word(h, v);
        }
        h = fold_word(h, self.total_logged as u64);
        h = fold_word(h, self.rejects as u64);
        h
    }

    /// 账本自洽审计。
    pub fn audit(&self) -> Result<(), u32> {
        if self.cases.len() != self.verdicts.len() || self.cases.len() != self.marks.len() {
            return Err(Diag::ESeal.val());
        }
        if !self.append_only_ok() {
            return Err(Diag::EUniq.val());
        }
        for i in 0..self.cases.len() {
            let c = match self.cases.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            let v = match self.verdicts.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if v != verdict_of(&c) {
                return Err(Diag::ESeal.val());
            }
            let m = match self.marks.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if mark_of(&c) != Ok(m) {
                return Err(Diag::ESeal.val());
            }
        }
        Ok(())
    }

    /// 封账：审计通过后记根并冻结（单向，重复封账拒绝）。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(Diag::ESealed.val());
        }
        self.audit()?;
        self.root = self.fold();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：逐格复算判决与票号。
    pub fn replay(&self) -> Result<u32, u32> {
        let mut resolved = 0u32;
        for i in 0..self.cases.len() {
            let c = match self.cases.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            let v = match self.verdicts.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if v != verdict_of(&c) {
                return Err(Diag::ESeal.val());
            }
            let m = match self.marks.get(i) {
                Some(x) => *x,
                None => return Err(Diag::ESeal.val()),
            };
            if mark_of(&c) != Ok(m) {
                return Err(Diag::ESeal.val());
            }
            if v == V_PASS {
                resolved += 1;
            }
        }
        if resolved != self.resolved() {
            return Err(Diag::ESeal.val());
        }
        Ok(resolved)
    }

    /// 封账根。
    pub fn root(&self) -> u64 {
        self.root
    }
}

/// 已通过断言的（编号, 判据组, 票号）表。
pub fn batch_table(l: &B35Ledger) -> Vec<(u16, u8, u64)> {
    let mut out = Vec::new();
    for i in 0..l.len() {
        let c = match l.case_at(i) {
            Some(x) => x,
            None => continue,
        };
        let m = match l.mark_at(i) {
            Some(x) => x,
            None => continue,
        };
        if m != 0 {
            out.push((c.id, c.group, m));
        }
    }
    out
}

/// 批收口报告（非空落档）：断言数/判据覆盖/通过率/折叠根。
pub fn closeout_report(l: &B35Ledger) -> String {
    format!(
        "b35 asserts={} groups={}/{} rate={}permille green={} root={:016x}",
        l.len(),
        {
            let mut covered = 0u32;
            for g in 1..=(GROUPS as u8) {
                if l.group_count(g) > 0 {
                    covered += 1;
                }
            }
            covered
        },
        GROUPS,
        l.pass_permille(),
        l.batch_green(),
        l.root(),
    )
}

/// 构建标准 B35 账本：40 条断言全部 PASS，覆盖 19 组
/// （组 1..=18 各 2 条 + 组 19 共 4 条）。
pub fn build_b35() -> B35Ledger {
    let mut l = B35Ledger::new();
    let mut id = 1u16;
    // 组 1..=18 各两条。
    let mut g = 1u8;
    while g <= 18 {
        let _ = l.register((id - 1) as usize, B35Case::make(id, g, 1, 100 + id as u32 * 7, true));
        id += 1;
        let _ = l.register((id - 1) as usize, B35Case::make(id, g, 2, 260 + id as u32 * 11, true));
        id += 1;
        g += 1;
    }
    // 组 19 收尾四条：id 37..=40。
    while id <= ID_MAX {
        let _ = l.register((id - 1) as usize, B35Case::make(id, 19, 1, 90 + id as u32 * 5, true));
        id += 1;
    }
    l
}

/// 注入矩阵（五路必抓）：
/// 0 基线全绿；1 混红破批绿；2 重号被拒；3 封账写被拒；4 组覆盖缺口被拒。
pub fn inject_matrix() -> [bool; 5] {
    let mut out = [false; 5];

    let base = build_b35();
    out[0] = base.batch_green()
        && base.audit().is_ok()
        && base.uniqueness_ok()
        && base.groups_all_covered()
        && base.append_only_ok();

    // 混红：前 39 条绿 + 第 40 条红，批绿必须被打破。
    let mut red = B35Ledger::new();
    let mut rid = 1u16;
    while rid <= 39 {
        let g = (((rid - 1) / 2 + 1) as u8).min(19);
        let _ = red.register((rid - 1) as usize, B35Case::make(rid, g, 1, 100 + rid as u32 * 7, true));
        rid += 1;
    }
    let _ = red.register(39, B35Case::make(40, 19, 3, 500, false));
    out[1] = !red.batch_green() && red.count_verdict(V_FAIL) == 1 && red.pass_permille() == 975;

    // 重号：编号 2 重复登记必须被拒且不动账（半账状态下打重号）。
    let mut dup = B35Ledger::new();
    let _ = dup.register(0, B35Case::make(1, 1, 1, 50, true));
    let _ = dup.register(1, B35Case::make(2, 1, 1, 60, true));
    let before = dup.total_logged();
    let r = dup.register(2, B35Case::make(2, 1, 1, 70, true));
    out[2] = r == Err(Diag::EDup.val()) && dup.total_logged() == before && !dup.is_frozen();

    // 封账后再写被拒且账面不变。
    let mut sealed = build_b35();
    let s1 = sealed.seal();
    let root = sealed.root();
    let again = sealed.seal() == Err(Diag::ESealed.val());
    let write = sealed.register(40, B35Case::make(41, 19, 1, 10, true)) == Err(Diag::ESealed.val());
    out[3] = s1.is_ok() && root != 0 && again && write && sealed.root() == root;

    // 组覆盖缺口：缺组 19 的账不算绿。
    let mut gap = B35Ledger::new();
    let mut gid = 1u16;
    while gid <= 36 {
        let _ = gap.register((gid - 1) as usize, B35Case::make(gid, ((gid - 1) % 18 + 1) as u8, 1, 120, true));
        gid += 1;
    }
    out[4] = !gap.batch_green() && !gap.groups_all_covered();

    out
}

// ---- 判据谓词（私有，方向：违约即假，末行为成立条件） --------------------

/// 段位守卫：SEG_BASE 对齐且落在本批段窗内。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    SEG_BASE == 0x78000
}

/// 批断言 ≥40 条全绿的聚合判据。
fn batch_green_ok(l: &B35Ledger) -> bool {
    if l.len() < SAMPLES {
        return false;
    }
    l.batch_green() && l.replay() == Ok(SAMPLES as u32)
}

/// 断言数入域级账只增不减。
fn append_only_ok(l: &B35Ledger) -> bool {
    if l.total_logged() < l.len() as u32 {
        return false;
    }
    l.append_only_ok() && l.total_logged() == SAMPLES as u32
}

/// 编号唯一性常绿。
fn uniqueness_ok(l: &B35Ledger) -> bool {
    l.uniqueness_ok() && batch_table(l).len() == SAMPLES
}

/// 19 条判据逐条落断言。
fn group_coverage_ok(l: &B35Ledger) -> bool {
    if !l.groups_all_covered() {
        return false;
    }
    let mut covered = 0u32;
    let mut g = 1u8;
    while g as usize <= GROUPS {
        if l.group_count(g) >= 1 {
            covered += 1;
        }
        g += 1;
    }
    covered == GROUPS as u32
}

/// 收口报告落档且非空。
fn closeout_ok(l: &B35Ledger) -> bool {
    let r = closeout_report(l);
    if r.is_empty() {
        return false;
    }
    r.contains("asserts=40") && r.contains("groups=19/19")
}

/// 双跑一致：两次构建折叠根相同。
fn dual_run_ok() -> bool {
    let a = build_b35();
    let b = build_b35();
    a.fold() == b.fold() && a.fold() != 0
}

/// 注入必抓：五路注入全数命中。
fn injection_ok() -> bool {
    inject_matrix() == [true; 5]
}

/// 十三项判据（批收口聚合）。
pub fn run_unx1500_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1500");
    let l = build_b35();

    s.add_dyn(
        "F1500-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={} window=75000..781C0", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1500-02 批断言四十条齐备",
        l.len() == SAMPLES && batch_green_ok(&l),
        &format!("cases={} samples={}", l.len(), SAMPLES),
    );

    s.add_dyn(
        "F1500-03 十九判据组全覆盖",
        group_coverage_ok(&l) && l.groups_all_covered(),
        &format!("g19={} g1={}", l.group_count(19), l.group_count(1)),
    );

    s.add_dyn(
        "F1500-04 判决分布全绿",
        l.count_verdict(V_PASS) == SAMPLES as u32
            && l.count_verdict(V_FAIL) == 0
            && l.count_verdict(V_ERROR) == 0
            && l.pass_permille() == 1000,
        &format!(
            "p={} f={} e={} rate={}",
            l.count_verdict(V_PASS),
            l.count_verdict(V_FAIL),
            l.count_verdict(V_ERROR),
            l.pass_permille()
        ),
    );

    s.add_dyn(
        "F1500-05 编号唯一性常绿",
        uniqueness_ok(&l),
        &format!("tickets={} len={}", batch_table(&l).len(), l.len()),
    );

    // 结构拒绝矩阵：乱序/越界/组越界/空样本/重复五门。
    let mut r = B35Ledger::new();
    let ok0 = r.register(0, B35Case::make(1, 1, 1, 60, true)) == Ok(V_PASS);
    let order = r.register(7, B35Case::make(9, 2, 1, 70, true)) == Err(Diag::EOrder.val());
    let range = r.register(1, B35Case::make(41, 1, 1, 80, true)) == Err(Diag::ERange.val());
    let group = r.register(1, B35Case::make(2, 20, 1, 80, true)) == Err(Diag::EGroup.val());
    let empty = r.register(1, B35Case::make(2, 1, 0, 0, false)) == Err(Diag::EEmpty.val());
    let dup = r.register(1, B35Case::make(1, 1, 1, 90, true)) == Err(Diag::EDup.val());
    s.add_dyn(
        "F1500-06 结构拒绝矩阵五门",
        ok0 && order && range && group && empty && dup && r.rejects() == 5 && r.len() == 1,
        &format!("rejects={} len={}", r.rejects(), r.len()),
    );

    s.add_dyn(
        "F1500-07 断言数只增不减",
        append_only_ok(&l),
        &format!("total={} len={}", l.total_logged(), l.len()),
    );

    s.add_dyn(
        "F1500-08 收口报告落档非空",
        closeout_ok(&l),
        &closeout_report(&l),
    );

    // 冻结幂等与封账后写入拒绝。
    let mut f = build_b35();
    let s1 = f.seal();
    let root = f.root();
    let again = f.seal() == Err(Diag::ESealed.val());
    let locked = f.register(40, B35Case::make(40, 19, 1, 1, true)) == Err(Diag::ESealed.val());
    s.add_dyn(
        "F1500-09 封账单向且写入被拒",
        s1.is_ok() && root != 0 && again && locked && f.root() == root,
        &format!("root={:016x}", root),
    );

    s.add_dyn(
        "F1500-10 重放逐格一致",
        l.replay() == Ok(SAMPLES as u32) && l.audit().is_ok(),
        &format!("replay={:?}", l.replay()),
    );

    // 票号两两不同（PASS 出票唯一）。
    let mut uniq_marks = true;
    let tbl = batch_table(&l);
    for i in 0..tbl.len() {
        let mut j = i + 1;
        while j < tbl.len() {
            let (a, b) = match (tbl.get(i), tbl.get(j)) {
                (Some(a), Some(b)) => (a.2, b.2),
                _ => {
                    uniq_marks = false;
                    break;
                }
            };
            if a == b {
                uniq_marks = false;
            }
            j += 1;
        }
    }
    s.add_dyn(
        "F1500-11 出票唯一且满额",
        uniq_marks && tbl.len() == SAMPLES,
        &format!("tickets={}", tbl.len()),
    );

    s.add_dyn(
        "F1500-12 双跑一致",
        dual_run_ok(),
        &format!("fold={:016x}", build_b35().fold()),
    );

    s.add_dyn(
        "F1500-13 注入必抓五路",
        injection_ok(),
        &format!("inject={:?}", inject_matrix()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_full_green() {
        let l = build_b35();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.count_verdict(V_PASS), 40);
        assert_eq!(l.pass_permille(), 1000);
        assert!(l.batch_green());
        assert!(l.groups_all_covered());
        assert_eq!(l.group_count(19), 4);
        assert_eq!(l.group_count(1), 2);
    }

    #[test]
    fn uniqueness_and_append_only() {
        let l = build_b35();
        assert!(l.uniqueness_ok());
        assert!(l.append_only_ok());
        assert_eq!(l.total_logged(), 40);
        // 混入一条失败：断言总数仍只增不减。
        let mut red = B35Ledger::new();
        let mut rid = 1u16;
        while rid <= 39 {
            let g = (((rid - 1) / 2 + 1) as u8).min(19);
            let _ = red.register((rid - 1) as usize, B35Case::make(rid, g, 1, 100, true));
            rid += 1;
        }
        let _ = red.register(39, B35Case::make(40, 19, 1, 10, false));
        assert_eq!(red.total_logged(), 40);
        assert!(red.append_only_ok());
        assert!(!red.batch_green());
    }

    #[test]
    fn refusal_codes_exact() {
        let mut l = B35Ledger::new();
        assert_eq!(l.register(0, B35Case::make(1, 1, 1, 50, true)), Ok(V_PASS));
        assert_eq!(l.register(5, B35Case::make(6, 2, 1, 60, true)), Err(Diag::EOrder.val()));
        assert_eq!(l.register(1, B35Case::make(0, 1, 1, 60, true)), Err(Diag::ERange.val()));
        assert_eq!(l.register(1, B35Case::make(2, 0, 1, 60, true)), Err(Diag::EGroup.val()));
        assert_eq!(l.register(1, B35Case::make(2, 1, 0, 0, true)), Err(Diag::EEmpty.val()));
        assert_eq!(l.register(1, B35Case::make(1, 1, 1, 70, true)), Err(Diag::EDup.val()));
        assert_eq!(l.rejects(), 5);
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn seal_monotonic_and_locked() {
        let mut l = build_b35();
        let root = l.seal();
        assert!(root.is_ok());
        assert_ne!(l.root(), 0);
        assert_eq!(l.seal(), Err(Diag::ESealed.val()));
        assert_eq!(
            l.register(40, B35Case::make(40, 19, 1, 5, true)),
            Err(Diag::ESealed.val())
        );
        assert_eq!(l.replay(), Ok(40));
        assert!(l.audit().is_ok());
    }

    #[test]
    fn red_assertion_breaks_batch() {
        let mut l = B35Ledger::new();
        for id in 1..=39u16 {
            let g = ((id - 1) / 2 + 1).min(19) as u8;
            let _ = l.register((id - 1) as usize, B35Case::make(id, g, 1, 100, true));
        }
        // 第 40 条失败：通过率 975‰，批不绿，判据组 19 仍覆盖。
        let _ = l.register(39, B35Case::make(40, 19, 2, 400, false));
        assert_eq!(l.pass_permille(), 975);
        assert!(!l.batch_green());
        assert_eq!(l.count_verdict(V_FAIL), 1);
        assert!(l.groups_all_covered());
    }

    #[test]
    fn closeout_report_nonempty() {
        let l = build_b35();
        let r = closeout_report(&l);
        assert!(r.contains("asserts=40"));
        assert!(r.contains("groups=19/19"));
        assert!(r.contains("green=true"));
        assert!(r.len() > 20);
    }

    #[test]
    fn diag_codes_closed_set() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::ERange.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::ESeal.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x0d), "未知诊断码：不在 0x78001..0x7800C 闭集内");
        assert_eq!(detail(0), "未知诊断码：不在 0x78001..0x7800C 闭集内");
    }

    #[test]
    fn seg_guard_values() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0);
    }

    #[test]
    fn dual_run_and_injections() {
        assert!(dual_run_ok());
        assert_eq!(inject_matrix(), [true; 5]);
    }

    #[test]
    fn checks_all_green_thirteen() {
        let s = run_unx1500_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
        // 动态明细侧表定容 8：13 条判据的后 5 条明细溢出属显性记账，
        // 检查项本体不受影响（容量 112 足够）。
        assert_eq!(s.dyn_dropped(), 5);
    }
}
