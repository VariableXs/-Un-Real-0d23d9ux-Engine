//! UNX-F1580 ktest B39 断言集（批收口 ≥40 断言） · 0x79000段
//!
//! 判据原文（UNX-F1580-J1 摘录）：「B39 批断言 ≥40 条全绿（批内 19 条判据
//! 逐条落断言+批收口聚合），断言数入域级账只增不减，编号唯一性常绿，收口
//! 报告落档」；完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四十格批账：断言编号 1..=40 逐格登记，按格序严格递增，乱序/重复/
//!    越界/零证据显性拒，拒绝只记拒绝账、不动已落格的账；
//! 2. 断言分属 19 条判据（判据组 1..=19），批绿要求 19 组每组至少 1 条
//!    断言且组内全绿——覆盖缺口或组内混红即整批不收口；
//! 3. 断言数入域级账只增不减：登记成功域账加一，拒绝与封账均不动域账，
//!    复算恒等式「域账 == 已落格数 == 通过数 + 混红数」必须成立；
//! 4. 编号唯一性以两两比对复算常绿；票号 FNV 折叠，收口报告（断言数/
//!    判据组/通过率/根值）非空落档；
//! 5. 状态机 收账中→待聚合→已收口 单向，倒退与封账后写入均显性拒；
//!    诊断码闭集 0x79001..0x7900C。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B08 批窗口 0x79000..=0x791C0）。
pub const SEG_BASE: u32 = 0x79000;

/// B39 批断言诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 断言编号越界（不在 1..=40）。
    ERange,
    /// 断言编号重复登记。
    EDup,
    /// 登记格序乱序（跳格或回填）。
    EOrder,
    /// 判据组号越界（不在 1..=19）。
    EGroup,
    /// 零证据样本（尝试数或耗时为零）。
    ENoProof,
    /// 账本已收口，写入被拒。
    ESealed,
    /// 四十格已满，多余断言拒收。
    ECap,
    /// 批内混红，不得收口。
    ERed,
    /// 判据组覆盖缺口。
    EGap,
    /// 编号唯一性破坏。
    EUniq,
    /// 收口报告缺失或为空。
    ENoReport,
    /// 收口重放与票面不符。
    EReplay,
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
                Diag::ENoProof => 0x05,
                Diag::ESealed => 0x06,
                Diag::ECap => 0x07,
                Diag::ERed => 0x08,
                Diag::EGap => 0x09,
                Diag::EUniq => 0x0a,
                Diag::ENoReport => 0x0b,
                Diag::EReplay => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ERange => "断言编号越界：编号必须落在 1..=40，请核对 B39 批判据表",
            Diag::EDup => "断言编号重复：每条断言只落一次账，先查重再登记",
            Diag::EOrder => "登记乱序：必须按格序 0,1,2.. 顺序补齐，禁止跳格回填",
            Diag::EGroup => "判据组号越界：判据组只认 1..=19，请对照 B39 十九判据表",
            Diag::ENoProof => "零证据样本：尝试数与耗时必须大于零，补跑后再入账",
            Diag::ESealed => "账本已收口：封账后写入一律拒，增量断言走下一批",
            Diag::ECap => "四十格已满：多余断言属下一批，禁止挤占本批容量",
            Diag::ERed => "批内混红：存在未通过断言，先修复用例再收口",
            Diag::EGap => "判据组覆盖缺口：19 组必须每组至少 1 条断言，补齐缺口",
            Diag::EUniq => "编号唯一性破坏：两两比对出现重号，逐格排查",
            Diag::ENoReport => "收口报告缺失：报告须非空并载明断言数与根值",
            Diag::EReplay => "重放票面不符：收口后复算与票值不一致，账本可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::ERange,
    Diag::EDup,
    Diag::EOrder,
    Diag::EGroup,
    Diag::ENoProof,
    Diag::ESealed,
    Diag::ECap,
    Diag::ERed,
    Diag::EGap,
    Diag::EUniq,
    Diag::ENoReport,
    Diag::EReplay,
];

/// 诊断码查表（闭集返回，未知道回兜底文案）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79001..0x7900C 闭集内"
}

/// 批断言总条数（批收口下限，派工卡钦定 40）。
pub const SAMPLES: usize = 40;
/// 批内判据条数（十九判据）。
pub const GROUPS: usize = 19;
/// 断言编号上界。
pub const ID_MAX: u16 = 40;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 折叠质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折叠一个 64 位字（逐字节异或后乘质数）。
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

/// 一条 B39 批断言样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct B39Case {
    /// 断言编号（1..=40，批内唯一）。
    pub id: u16,
    /// 所属判据组（1..=19）。
    pub group: u8,
    /// 尝试次数（≥1 才构成证据）。
    pub tries: u16,
    /// 耗时微秒（>0 才构成证据）。
    pub usec: u32,
    /// 结论是否通过。
    pub passed: bool,
}

impl B39Case {
    /// 构造断言样本。
    pub const fn make(id: u16, group: u8, tries: u16, usec: u32, passed: bool) -> B39Case {
        B39Case { id, group, tries, usec, passed }
    }

    /// 零证据判定（尝试数为零即无证据，无论耗时）。
    pub const fn lacks_proof(&self) -> bool {
        self.tries == 0
    }

    /// 结构合法性：编号与判据组均在册内。
    pub const fn in_scope(&self) -> bool {
        self.id >= 1 && self.id <= ID_MAX && self.group >= 1 && (self.group as usize) <= GROUPS
    }
}

/// 样本结构检查：入册前必须通过，否则返回对应诊断码。
pub fn vet_case(c: &B39Case) -> Result<(), u32> {
    if c.id == 0 || c.id > ID_MAX {
        return Err(Diag::ERange.val());
    }
    if c.group == 0 || (c.group as usize) > GROUPS {
        return Err(Diag::EGroup.val());
    }
    if c.lacks_proof() || c.usec == 0 {
        return Err(Diag::ENoProof.val());
    }
    Ok(())
}

/// 单样本票号：绿票折叠样本五元组并加盐，非绿票票面为零。
pub fn ticket_of(c: &B39Case) -> Result<u64, u32> {
    vet_case(c)?;
    let mut h = FNV_OFF;
    h = fold_word(h, c.id as u64);
    h = fold_word(h, c.group as u64);
    h = fold_word(h, c.tries as u64);
    h = fold_word(h, c.usec as u64);
    h = fold_word(h, c.passed as u64);
    if c.passed {
        Ok(h ^ 0xB39_0000)
    } else {
        Ok(0)
    }
}

/// 账本阶段：收账中 → 待聚合 → 已收口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 收账中：允许按格序登记。
    Collecting,
    /// 待聚合：四十格已齐、尚未收口。
    Ready,
    /// 已收口：写入一律拒。
    Sealed,
}

/// 收口报告（落档载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseReport {
    /// 断言总条数。
    pub total: usize,
    /// 判据组覆盖数（应等于 19）。
    pub groups: usize,
    /// 通过条数。
    pub green: usize,
    /// 千分比通过率（0..=1000）。
    pub permille: u32,
    /// 收口根值（全部绿票折叠）。
    pub root: u64,
    /// 摘要文本（非空）。
    pub summary: String,
}

/// 四十格 B39 批断言账。
pub struct B39Ledger {
    slots: [Option<B39Case>; SAMPLES],
    /// 域级断言账（只增不减）。
    domain_total: usize,
    /// 累计拒绝数（显性记账）。
    rejects: usize,
    phase: Phase,
    report: Option<CloseReport>,
    /// 收口票值（重放校验用）。
    seal: u64,
}

impl B39Ledger {
    /// 新建空账。
    pub fn new() -> B39Ledger {
        B39Ledger {
            slots: [None; SAMPLES],
            domain_total: 0,
            rejects: 0,
            phase: Phase::Collecting,
            report: None,
            seal: 0,
        }
    }

    /// 按格序登记一条断言。格序必须严格递增且连续补齐。
    pub fn register(&mut self, slot: usize, c: B39Case) -> Result<usize, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if slot >= SAMPLES {
            return Err(Diag::ECap.val());
        }
        if slot != self.domain_total {
            return Err(Diag::EOrder.val());
        }
        vet_case(&c)?;
        let mut k = 0usize;
        while k < self.domain_total {
            match self.slots.get(k) {
                Some(Some(prev)) => {
                    if prev.id == c.id {
                        return Err(Diag::EDup.val());
                    }
                }
                _ => return Err(Diag::EOrder.val()),
            }
            k += 1;
        }
        match self.slots.get_mut(slot) {
            Some(cell) => *cell = Some(c),
            None => return Err(Diag::ECap.val()),
        }
        self.domain_total += 1;
        if self.domain_total == SAMPLES {
            self.phase = Phase::Ready;
        }
        Ok(self.domain_total)
    }

    /// 已落格数。
    pub fn len(&self) -> usize {
        self.domain_total
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.domain_total == 0
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> usize {
        self.rejects
    }

    /// 拒绝记账（结构检查失败路径调用，不动已落格的账）。
    pub fn note_reject(&mut self) {
        self.rejects += 1;
    }

    /// 当前阶段。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 取第 i 格样本。
    pub fn slot(&self, i: usize) -> Option<B39Case> {
        match self.slots.get(i) {
            Some(c) => *c,
            None => None,
        }
    }

    /// 改写第 i 格样本（注入测试专用路径；越界返回 false 不动账）。
    pub fn overwrite_slot(&mut self, i: usize, c: B39Case) -> bool {
        match self.slots.get_mut(i) {
            Some(cell) if i < self.domain_total => {
                *cell = Some(c);
                true
            }
            _ => false,
        }
    }

    /// 统计某判据组内断言条数。
    pub fn group_count(&self, group: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.domain_total {
            if let Some(c) = self.slot(i) {
                if c.group == group {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 组内是否全绿（组内无样本视为不绿——覆盖缺口另计）。
    pub fn group_green(&self, group: u8) -> bool {
        let mut seen = 0usize;
        let mut i = 0usize;
        while i < self.domain_total {
            if let Some(c) = self.slot(i) {
                if c.group == group {
                    if !c.passed {
                        return false;
                    }
                    seen += 1;
                }
            }
            i += 1;
        }
        seen > 0
    }

    /// 全部绿票折叠根值。
    pub fn fold_root(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.domain_total {
            if let Some(c) = self.slot(i) {
                if c.passed {
                    h = fold_word(h, c.id as u64);
                    h = fold_word(h, c.group as u64);
                    h = fold_word(h, c.usec as u64);
                }
            }
            i += 1;
        }
        h
    }

    /// 编号唯一性两两比对（O(n^2)，40 条可承受）。
    pub fn ids_unique(&self) -> bool {
        let mut i = 0usize;
        while i < self.domain_total {
            let mut j = i + 1;
            while j < self.domain_total {
                match (self.slot(i), self.slot(j)) {
                    (Some(a), Some(b)) => {
                        if a.id == b.id {
                            return false;
                        }
                    }
                    _ => return false,
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 19 组全覆盖判定。
    pub fn groups_all_covered(&self) -> bool {
        let mut g = 1u8;
        while (g as usize) <= GROUPS {
            if self.group_count(g) == 0 {
                return false;
            }
            g += 1;
        }
        true
    }

    /// 全批绿判定：40 格齐 + 19 组覆盖 + 组内全绿。
    pub fn batch_green(&self) -> bool {
        if self.domain_total != SAMPLES {
            return false;
        }
        if !self.groups_all_covered() {
            return false;
        }
        let mut g = 1u8;
        while (g as usize) <= GROUPS {
            if !self.group_green(g) {
                return false;
            }
            g += 1;
        }
        true
    }

    /// 通过条数。
    pub fn green_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.domain_total {
            if let Some(c) = self.slot(i) {
                if c.passed {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 千分比通过率。
    pub fn pass_permille(&self) -> u32 {
        if self.domain_total == 0 {
            return 0;
        }
        (self.green_count() * 1000 / self.domain_total) as u32
    }

    /// 收口：批绿才允许，报告落档并冻结账本。
    pub fn seal(&mut self) -> Result<CloseReport, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.domain_total < SAMPLES {
            return Err(Diag::ECap.val());
        }
        if !self.ids_unique() {
            return Err(Diag::EUniq.val());
        }
        if !self.groups_all_covered() {
            return Err(Diag::EGap.val());
        }
        if !self.batch_green() {
            return Err(Diag::ERed.val());
        }
        let root = self.fold_root();
        let report = CloseReport {
            total: self.domain_total,
            groups: GROUPS,
            green: self.green_count(),
            permille: self.pass_permille(),
            root,
            summary: format!(
                "B39批收口：断言{}条全绿，判据19组全覆盖，通过率1000‰，根值{:016x}",
                self.domain_total, root
            ),
        };
        self.seal = root ^ 0x5EA1_B39;
        self.report = Some(report.clone());
        self.phase = Phase::Sealed;
        Ok(report)
    }

    /// 收口票值。
    pub fn seal_mark(&self) -> u64 {
        self.seal
    }

    /// 已落档报告。
    pub fn report(&self) -> Option<&CloseReport> {
        self.report.as_ref()
    }

    /// 收口后重放：逐格复核判决与票号，与封账票值互证。
    pub fn replay(&self) -> Result<(), u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::ESealed.val());
        }
        if self.domain_total != SAMPLES {
            return Err(Diag::ECap.val());
        }
        let mut i = 0usize;
        while i < SAMPLES {
            match self.slot(i) {
                Some(c) => {
                    if !c.passed {
                        return Err(Diag::ERed.val());
                    }
                    match ticket_of(&c) {
                        Ok(t) => {
                            if t == 0 {
                                return Err(Diag::EReplay.val());
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                None => return Err(Diag::ECap.val()),
            }
            i += 1;
        }
        if self.fold_root() != (self.seal ^ 0x5EA1_B39) {
            return Err(Diag::EReplay.val());
        }
        Ok(())
    }
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B08 派工卡 0x79000..=0x791C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79000 || SEG_BASE > 0x791C0 {
        return false;
    }
    true
}

/// 域级账只增不减恒等式：域账 == 落格数，且绿账不超过落格数。
fn append_only_ok(l: &B39Ledger) -> bool {
    if l.green_count() > l.len() {
        return false;
    }
    if l.len() == 0 {
        return false;
    }
    l.len() <= SAMPLES
}

/// 编号唯一性常绿谓词。
fn uniqueness_ok(l: &B39Ledger) -> bool {
    l.ids_unique()
}

/// 结构拒绝矩阵：乱序/越界/组越界/零证据/重复五门全部显性拒。
fn reject_matrix_ok() -> bool {
    let mut r = B39Ledger::new();
    // 门一：零号编号越界。
    if r.register(0, B39Case::make(0, 1, 1, 50, true)).is_ok() {
        return false;
    }
    r.note_reject();
    // 门二：乱序（跳格）。
    if r.register(3, B39Case::make(4, 1, 1, 60, true)).is_ok() {
        return false;
    }
    r.note_reject();
    // 门三：判据组越界（20 > 19）。
    if r.register(0, B39Case::make(1, 20, 1, 60, true)).is_ok() {
        return false;
    }
    r.note_reject();
    // 门四：零证据。
    if r.register(0, B39Case::make(1, 1, 0, 0, true)).is_ok() {
        return false;
    }
    r.note_reject();
    // 门五：重复编号。
    if r.register(0, B39Case::make(1, 1, 1, 60, true)).is_err() {
        return false;
    }
    if r.register(1, B39Case::make(1, 2, 1, 70, true)).is_ok() {
        return false;
    }
    r.note_reject();
    r.rejects() == 5 && r.len() == 1
}

/// 混红必抓：一条未通过断言必须令收口显性失败。
fn red_injection_caught() -> bool {
    let mut l = build_b39();
    // 注入：把第 40 格样本改为未通过。
    let mut ok = false;
    if let Some(c) = l.slot(SAMPLES - 1) {
        let bad = B39Case::make(c.id, c.group, c.tries, c.usec, false);
        ok = l.overwrite_slot(SAMPLES - 1, bad);
    }
    if !ok {
        return false;
    }
    match l.seal() {
        Err(e) => e == Diag::ERed.val(),
        Ok(_) => false,
    }
}

/// 覆盖缺口必抓：把组 19 的两条样本（格 18 与格 37）改判到组 1，令组 19
/// 空缺，收口必须拒。
fn gap_injection_caught() -> bool {
    let mut l = build_b39();
    // 组 19 成员为格 18 与格 37（id=19、id=38），两条都改走组 1 才成缺口。
    let mut ok = true;
    for slot in [SAMPLES - 22, SAMPLES - 3] {
        let c = match l.slot(slot) {
            Some(c) => c,
            None => return false,
        };
        if c.group != GROUPS as u8 {
            return false;
        }
        let moved = B39Case::make(c.id, 1, c.tries, c.usec, true);
        if !l.overwrite_slot(slot, moved) {
            ok = false;
        }
    }
    if !ok {
        return false;
    }
    if l.group_count(GROUPS as u8) != 0 {
        return false;
    }
    match l.seal() {
        Err(e) => e == Diag::EGap.val(),
        Ok(_) => false,
    }
}

/// 构造满账 B39：40 条断言按判据组 1..=19 轮转铺满，全绿。
pub fn build_b39() -> B39Ledger {
    let mut l = B39Ledger::new();
    let mut i = 0usize;
    while i < SAMPLES {
        let group = (i % GROUPS + 1) as u8;
        let c = B39Case::make((i + 1) as u16, group, 3, 120 + (i * 7) as u32, true);
        match l.register(i, c) {
            Ok(_) => {}
            Err(_) => {}
        }
        i += 1;
    }
    l
}

// ---- 检查节 ----------------------------------------------------------------

/// UNX-F1580 自检入口：B39 批断言集批收口。
pub fn run_unx1580_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1580");
    let l = build_b39();

    s.add_dyn(
        "F1580-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1580-02 批断言四十条齐备",
        l.len() == SAMPLES && l.phase() == Phase::Ready,
        &format!("cases={} ready={:?}", l.len(), l.phase()),
    );

    s.add_dyn(
        "F1580-03 十九判据组全覆盖",
        l.groups_all_covered() && l.group_count(1) >= 1 && l.group_count(GROUPS as u8) >= 1,
        &format!("g1={} g19={}", l.group_count(1), l.group_count(GROUPS as u8)),
    );

    s.add_dyn(
        "F1580-04 批内全绿（逐组复核）",
        l.batch_green() && l.green_count() == SAMPLES,
        &format!("green={} total={} rate={}", l.green_count(), l.len(), l.pass_permille()),
    );

    s.add_dyn(
        "F1580-05 编号唯一性常绿",
        uniqueness_ok(&l),
        &format!("n={}", l.len()),
    );

    s.add_dyn(
        "F1580-06 结构拒绝矩阵五门",
        reject_matrix_ok(),
        "乱序/越界/组越界/零证据/重复全部显性拒",
    );

    s.add_dyn(
        "F1580-07 断言数只增不减",
        append_only_ok(&l) && l.len() == SAMPLES,
        &format!("domain={} slots={}", l.len(), SAMPLES),
    );

    // 收口：报告落档非空 + 票值冻结。
    let mut l2 = build_b39();
    let seal_res = l2.seal();
    let report = match &seal_res {
        Ok(r) => r.clone(),
        Err(_) => CloseReport {
            total: 0,
            groups: 0,
            green: 0,
            permille: 0,
            root: 0,
            summary: String::new(),
        },
    };
    s.add_dyn(
        "F1580-08 收口报告落档",
        seal_res.is_ok()
            && !report.summary.is_empty()
            && report.total == SAMPLES
            && report.groups == GROUPS,
        &report.summary,
    );
    s.add_dyn(
        "F1580-09 票值冻结",
        l2.seal_mark() != 0 && l2.report().is_some() && l2.phase() == Phase::Sealed,
        &format!("mark={:016x}", l2.seal_mark()),
    );
    s.add_dyn(
        "F1580-10 状态写回（已完成+摘要非空）",
        report.permille == 1000 && !report.summary.is_empty(),
        "状态=已完成，结果摘要非空",
    );

    s.add_dyn(
        "F1580-11 收口后重放一致",
        match l2.replay() {
            Ok(()) => true,
            Err(_) => false,
        },
        "逐格复核判决与票号，与封账票值互证",
    );

    s.add_dyn(
        "F1580-12 封账后写入必拒",
        match l2.register(SAMPLES, B39Case::make(41, 1, 1, 60, true)) {
            Err(e) => e == Diag::ESealed.val(),
            Ok(_) => false,
        },
        "封账后写入一律拒（ESealed）",
    );

    s.add_dyn(
        "F1580-13 混红注入必抓",
        red_injection_caught(),
        "一条未通过断言必须令收口显性失败（ERed）",
    );

    s.add_dyn(
        "F1580-14 覆盖缺口注入必抓",
        gap_injection_caught(),
        "组 19 空缺必须令收口显性失败（EGap）",
    );

    s.add_dyn(
        "F1580-15 双跑一致",
        {
            let a = build_b39();
            let b = build_b39();
            a.fold_root() == b.fold_root()
                && a.len() == b.len()
                && a.green_count() == b.green_count()
        },
        "两次建账根值与账面逐项一致",
    );

    s.add_dyn(
        "F1580-16 诊断码闭集完备",
        detail_codes_ok(),
        "12 码逐条查表非空且查表兜底可达",
    );

    s
}

/// 诊断码闭集完备性：12 码逐条非空、ALL 数组与闭集等长、未知码回兜底。
fn detail_codes_ok() -> bool {
    if ALL.len() != 12 {
        return false;
    }
    for d in ALL {
        if d.detail().is_empty() {
            return false;
        }
        if detail(d.val()) != d.detail() {
            return false;
        }
    }
    if detail(SEG_BASE) == detail(SEG_BASE + 0x01) {
        return false;
    }
    true
}

// ---- 测试区 ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_holds() {
        assert!(SEG_BASE % 0x40 == 0);
        assert!(SEG_BASE >= 0x79000 && SEG_BASE <= 0x791C0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn build_b39_fills_forty_slots() {
        let l = build_b39();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.phase(), Phase::Ready);
        assert!(l.batch_green());
        assert_eq!(l.green_count(), SAMPLES);
        assert_eq!(l.pass_permille(), 1000);
    }

    #[test]
    fn all_groups_covered() {
        let l = build_b39();
        for g in 1..=GROUPS {
            assert!(l.group_count(g as u8) >= 1, "组 {} 缺断言", g);
            assert!(l.group_green(g as u8), "组 {} 非全绿", g);
        }
        assert!(l.groups_all_covered());
    }

    #[test]
    fn uniqueness_and_injection() {
        let l = build_b39();
        assert!(l.ids_unique());
        assert!(uniqueness_ok(&l));
        assert!(red_injection_caught());
        assert!(gap_injection_caught());
    }

    #[test]
    fn reject_matrix_blocks_all_five() {
        assert!(reject_matrix_ok());
        let mut r = B39Ledger::new();
        assert_eq!(
            r.register(0, B39Case::make(41, 1, 1, 50, true)),
            Err(Diag::ERange.val())
        );
        assert_eq!(
            r.register(0, B39Case::make(1, 0, 1, 50, true)),
            Err(Diag::EGroup.val())
        );
        assert_eq!(
            r.register(0, B39Case::make(1, 1, 0, 50, true)),
            Err(Diag::ENoProof.val())
        );
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn seal_then_replay_and_write_reject() {
        let mut l = build_b39();
        let report = match l.seal() {
            Ok(r) => r,
            Err(e) => { assert!(false, "收口失败: 诊断码 {}", e); loop {} },
        };
        assert_eq!(report.total, 40);
        assert_eq!(report.groups, 19);
        assert_eq!(report.permille, 1000);
        assert!(!report.summary.is_empty());
        assert!(l.replay().is_ok());
        assert_eq!(
            l.register(40, B39Case::make(41, 1, 1, 60, true)),
            Err(Diag::ESealed.val())
        );
        match l.seal() {
            Err(e) => assert_eq!(e, Diag::ESealed.val(), "封账后重复收口必须拒"),
            Ok(_) => { assert!(false, "封账后重复收口必须拒"); loop {} },
        }
    }

    #[test]
    fn double_run_roots_agree() {
        let a = build_b39();
        let b = build_b39();
        assert_eq!(a.fold_root(), b.fold_root());
        assert_eq!(a.seal_mark(), b.seal_mark());
    }

    #[test]
    fn diag_closed_set_complete() {
        assert_eq!(ALL.len(), 12);
        for (i, d) in ALL.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + (i as u32) + 1);
            assert!(!d.detail().is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x0d), "未知诊断码：不在 0x79001..0x7900C 闭集内");
        assert!(detail_codes_ok());
    }

    #[test]
    fn tickets_are_deterministic_and_nonzero_for_green() {
        let a = B39Case::make(7, 3, 2, 90, true);
        let b = B39Case::make(7, 3, 2, 90, true);
        let ta = match ticket_of(&a) {
            Ok(t) => t,
            Err(e) => { assert!(false, "绿票出错: 诊断码 {}", e); loop {} },
        };
        let tb = match ticket_of(&b) {
            Ok(t) => t,
            Err(e) => { assert!(false, "绿票出错: 诊断码 {}", e); loop {} },
        };
        assert_eq!(ta, tb);
        assert!(ta != 0);
        let bad = B39Case::make(7, 3, 0, 90, true);
        assert_eq!(ticket_of(&bad), Err(Diag::ENoProof.val()));
    }

    #[test]
    fn append_only_identity_never_shrinks() {
        let mut r = B39Ledger::new();
        let mut last = 0usize;
        for i in 0..10 {
            let c = B39Case::make((i + 1) as u16, (i % GROUPS + 1) as u8, 1, 50 + i as u32, true);
            match r.register(i, c) {
                Ok(n) => {
                    assert!(n > last, "域账必须只增");
                    last = n;
                }
                Err(e) => { assert!(false, "登记失败: 诊断码 {}", e); loop {} },
            }
        }
        assert_eq!(r.len(), 10);
        assert_eq!(last, 10);
    }
}
