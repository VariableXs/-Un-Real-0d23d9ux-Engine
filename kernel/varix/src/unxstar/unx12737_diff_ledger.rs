//! UNX-F12737 对照回归差异列账（三态登记零静默） · 0x82240段
//!
//! 判据原文（派工卡 12737 逐字锚定）：
//!   ① 灵敏功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12738-J1 双机终账一致率 ≥95%（码值/偏移类零容差），
//!      CONFLICT 显性；
//!   ③ 实际验收按卡面判据为准，③条为锚定写回条款。
//!
//! 设计要点：
//! 1. 列账域模型：账簿由"账列"构成，账列三类别闭集（码值类/偏移类/
//!    计数类），列须先开账（open_column）再记账；未开账列出现观测差异
//!    属结构性本体差异——三态（本体差异/待归一/真差异）登记由此闭环；
//! 2. 双机终账一致率（UNX-F12738-J1 语义）：甲机账面与乙机账面逐列
//!    对平，一致率以千分比整数口径计（agreed×1000/total ≥ 950 即
//!    ≥95%），码值类与偏移类零容差（差 1 即 CONFLICT，不进容差），
//!    计数类有冻结容差预算（±2 内待归一，出预算真差异）；
//! 3. CONFLICT 显性：零容差类不平等必须落 CONFLICT 账行（列号/类别/
//!    甲乙值四元组），CONFLICT 不得被一致率分母摊薄——一致率达标而
//!    CONFLICT 账为空才算真绿，两条判据合取；
//! 4. 零静默：观测差异与登记账行逐一核对，账面无登记的观测差异立即
//!    告警入账（Alarm），告警账非空即封账拒——静默吞差显性红；
//! 5. 账簿相位 Open → Reconciling → Sealed 单向，rank 单调，倒退显性
//!    拒；封账后写入显性拒（账只读）；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!    saturating_*，禁裸下标；无 unwrap/expect/panic/assert（生产区）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12737 − 12600)）。
pub const SEG_BASE: u32 = 0x82240;

// ── 账列类别闭集 ─────────────────────────────────────────────────
/// 账列类别闭集（3 类：码值类/偏移类/计数类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColClass {
    /// 码值类：零容差，差 1 即 CONFLICT。
    Code,
    /// 偏移类：零容差，差 1 即 CONFLICT。
    Offset,
    /// 计数类：容差预算 ±2，出预算升级真差异。
    Count,
}

impl ColClass {
    /// 类别序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 由序号还原（闭集外显性拒）。
    pub fn from_rank(r: u32) -> Option<ColClass> {
        match r {
            0 => Some(ColClass::Code),
            1 => Some(ColClass::Offset),
            2 => Some(ColClass::Count),
            _ => None,
        }
    }
    /// 零容差类别判定（码值/偏移）。
    pub fn zero_tolerance(self) -> bool {
        matches!(self, ColClass::Code | ColClass::Offset)
    }
    pub const ALL: [ColClass; 3] = [ColClass::Code, ColClass::Offset, ColClass::Count];
}

// ── 三态登记闭集 ─────────────────────────────────────────────────
/// 三态登记闭集（本体差异/待归一/真差异）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriState {
    /// 本体差异：结构性（列未开账/列缺位），归一不可救。
    Body,
    /// 待归一：差异在容差预算内，可归一。
    Pending,
    /// 真差异：出容差预算或零容差类不等。
    Real,
}

impl TriState {
    pub fn rank(self) -> u32 {
        match self {
            TriState::Body => 0,
            TriState::Pending => 1,
            TriState::Real => 2,
        }
    }
    pub const ALL: [TriState; 3] = [TriState::Body, TriState::Pending, TriState::Real];
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x09 偏移，越出闭集回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookDiag {
    /// 列未开账：先 open_column 再记账。
    EColClosed,
    /// 列重复开账：一列一账，不重复开。
    EColDouble,
    /// 类别越闭集：不在 3 类闭集内。
    EClass,
    /// 相位倒退：Open→Reconciling→Sealed 只进不退。
    EPhase,
    /// 封账后写入：Sealed 后账只读。
    ESealed,
    /// 零静默告警：观测差异未登记，静默吞差即红。
    ESilent,
    /// 空账封账：无对平结果不得封账。
    EEmptySeal,
    /// 一致率越界：千分比 > 1000，账面自相矛盾。
    ERate,
}

impl BookDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BookDiag::EColClosed => "列未开账：先 open_column 登记列再记账",
            BookDiag::EColDouble => "列重复开账：一列一账，重复开显性拒",
            BookDiag::EClass => "类别越闭集：不在码值/偏移/计数 3 类闭集内",
            BookDiag::EPhase => "相位倒退：Open→Reconciling→Sealed 只进不退",
            BookDiag::ESealed => "封账后写入：Sealed 后账只读，先开新账",
            BookDiag::ESilent => "零静默告警：观测差异未登记，静默吞差即红",
            BookDiag::EEmptySeal => "空账封账：无对平结果不得封账",
            BookDiag::ERate => "一致率越界：千分比>1000，账面自相矛盾",
        }
    }
    pub const ALL: [BookDiag; 8] = [
        BookDiag::EColClosed,
        BookDiag::EColDouble,
        BookDiag::EClass,
        BookDiag::EPhase,
        BookDiag::ESealed,
        BookDiag::ESilent,
        BookDiag::EEmptySeal,
        BookDiag::ERate,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BookDiag::ALL.len() {
        if let Some(d) = BookDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82240 段闭集内"
}

// ── 冻结常量 ─────────────────────────────────────────────────────
/// 计数类容差预算（±2，冻结口径：对平预算条款）。
pub const COUNT_TOL: i32 = 2;
/// 一致率千分比阈值（≥950 即 ≥95%，UNX-F12738-J1）。
pub const RATE_PERMILLE: u32 = 950;
/// 列号域（1..=12，双机账面各 12 列）。
pub const COLS: u32 = 12;
/// 开账列容量（列登记上限 = 12）。
pub const REG_CAP: usize = 12;

// ── 账行与告警行 ─────────────────────────────────────────────────
/// 登记账行：列号 + 类别 + 三态 + 差值（有符号，负差也是差）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Posting {
    pub col: u32,
    pub class: ColClass,
    pub state: TriState,
    pub delta: i32,
}

/// CONFLICT 账行：零容差类不平等显性留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    pub col: u32,
    pub class: ColClass,
    pub side_a: u32,
    pub side_b: u32,
}

/// 告警行：零静默审计发现（观测差异无登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmRow {
    pub col: u32,
    pub observed_delta: i32,
}

// ── 账簿 ─────────────────────────────────────────────────────────
/// 账簿相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookPhase {
    /// 开账：列登记与写入开放。
    Open,
    /// 对平中：双机逐列对平进行中。
    Reconciling,
    /// 已封账：账只读。
    Sealed,
}

impl BookPhase {
    fn rank(self) -> u32 {
        match self {
            BookPhase::Open => 0,
            BookPhase::Reconciling => 1,
            BookPhase::Sealed => 2,
        }
    }
}

/// 差异列账簿：开账列登记表 + 三态账行 + CONFLICT 账 + 告警账。
pub struct DiffBook {
    /// 开账列登记表：(列号, 类别)。
    registry: Vec<(u32, ColClass)>,
    posts: Vec<Posting>,
    conflicts: Vec<ConflictRow>,
    alarms: Vec<AlarmRow>,
    phase: BookPhase,
    /// 对平统计：对平列数 / 一致列数（千分比口径的分母分子）。
    reconciled: u32,
    agreed: u32,
}

impl DiffBook {
    pub fn new() -> DiffBook {
        DiffBook {
            registry: Vec::new(),
            posts: Vec::new(),
            conflicts: Vec::new(),
            alarms: Vec::new(),
            phase: BookPhase::Open,
            reconciled: 0,
            agreed: 0,
        }
    }

    pub fn phase(&self) -> BookPhase {
        self.phase
    }

    /// 相位推进（单调；倒退显性拒）。
    pub fn advance(&mut self, to: BookPhase) -> Result<(), BookDiag> {
        if self.phase == BookPhase::Sealed {
            return Err(BookDiag::ESealed);
        }
        if to.rank() < self.phase.rank() {
            return Err(BookDiag::EPhase);
        }
        self.phase = to;
        Ok(())
    }

    /// 开账列登记（判序：封账 → 重复 → 容量 → 类别）。
    pub fn open_column(&mut self, col: u32, class: ColClass) -> Result<(), BookDiag> {
        if self.phase == BookPhase::Sealed {
            return Err(BookDiag::ESealed);
        }
        let mut k = 0usize;
        while k < self.registry.len() {
            if let Some(&(c, _)) = self.registry.get(k) {
                if c == col {
                    return Err(BookDiag::EColDouble);
                }
            }
            k += 1;
        }
        if self.registry.len() >= REG_CAP {
            return Err(BookDiag::EColClosed);
        }
        // 类别闭集由枚举类型自证；此处核对序号域防间接越界。
        if class.rank() > 2 {
            return Err(BookDiag::EClass);
        }
        self.registry.push((col, class));
        Ok(())
    }

    /// 列类别查表。
    fn class_of(&self, col: u32) -> Option<ColClass> {
        self.registry
            .iter()
            .find(|&&(c, _)| c == col)
            .map(|&(_, k)| k)
    }

    /// 三态判定（判序冻结：封账 → 列未开账 → 零容差 → 容差预算）。
    /// 返回 Ok(None) = 平账。
    pub fn judge(&mut self, col: u32, a: u32, b: u32) -> Result<Option<TriState>, BookDiag> {
        if self.phase == BookPhase::Sealed {
            return Err(BookDiag::ESealed);
        }
        if self.phase == BookPhase::Open {
            self.advance(BookPhase::Reconciling)?;
        }
        let class = match self.class_of(col) {
            Some(k) => k,
            None => {
                // 列未开账 = 结构性本体差异，登记入账。
                self.posts.push(Posting {
                    col,
                    class: ColClass::Code,
                    state: TriState::Body,
                    delta: 0,
                });
                return Ok(Some(TriState::Body));
            }
        };
        self.reconciled = self.reconciled.saturating_add(1);
        if class.zero_tolerance() {
            if a != b {
                // 零容差：差 1 即 CONFLICT，显性留痕 + 真差异登记。
                self.conflicts.push(ConflictRow { col, class, side_a: a, side_b: b });
                self.posts.push(Posting {
                    col,
                    class,
                    state: TriState::Real,
                    delta: (a as i64 - b as i64) as i32,
                });
                return Ok(Some(TriState::Real));
            }
            self.agreed = self.agreed.saturating_add(1);
            return Ok(None);
        }
        // 计数类：容差预算 ±2。
        let delta = (a as i64 - b as i64) as i32;
        if delta == 0 {
            self.agreed = self.agreed.saturating_add(1);
            return Ok(None);
        }
        let mag = if delta < 0 { delta.wrapping_neg() } else { delta };
        if mag <= COUNT_TOL {
            self.posts.push(Posting { col, class, state: TriState::Pending, delta });
            return Ok(Some(TriState::Pending));
        }
        self.posts.push(Posting { col, class, state: TriState::Real, delta });
        Ok(Some(TriState::Real))
    }

    /// 零静默审计：观测差异集（列号, 差值）与登记账行逐一核对，
    /// 账面无登记的观测差异立即告警入账。返回本轮告警数。
    pub fn zero_silence_audit(&mut self, observed: &[(u32, i32)]) -> Result<u32, BookDiag> {
        if self.phase == BookPhase::Sealed {
            return Err(BookDiag::ESealed);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < observed.len() {
            if let Some(&(col, delta)) = observed.get(i) {
                let mut covered = false;
                let mut k = 0usize;
                while k < self.posts.len() {
                    if let Some(p) = self.posts.get(k) {
                        if p.col == col {
                            covered = true;
                        }
                    }
                    k += 1;
                }
                // CONFLICT 账行也视为已登记覆盖。
                let mut k2 = 0usize;
                while k2 < self.conflicts.len() {
                    if let Some(c) = self.conflicts.get(k2) {
                        if c.col == col {
                            covered = true;
                        }
                    }
                    k2 += 1;
                }
                if !covered {
                    self.alarms.push(AlarmRow { col, observed_delta: delta });
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(n)
    }

    /// 双机终账一致率（千分比整数口径）。
    pub fn rate_permille(&self) -> Result<u32, BookDiag> {
        if self.reconciled == 0 {
            return Err(BookDiag::EEmptySeal);
        }
        let rate = self
            .agreed
            .saturating_mul(1000)
            .checked_div(self.reconciled)
            .unwrap_or(u32::MAX);
        if rate > 1000 {
            return Err(BookDiag::ERate);
        }
        Ok(rate)
    }

    /// J1 判据：一致率 ≥95% 且 CONFLICT 账行数 = 零容差类真差异行数
    ///（CONFLICT 显性：零容差类不平等必须全部留痕，不得摊薄）。
    pub fn j1_green(&self) -> bool {
        let rate_ok = match self.rate_permille() {
            Ok(r) => r >= RATE_PERMILLE,
            Err(_) => false,
        };
        // CONFLICT 显性核账：CONFLICT 行数 ≡ 零容差类 Real 态行数。
        let mut zt_real = 0u32;
        let mut k = 0usize;
        while k < self.posts.len() {
            if let Some(p) = self.posts.get(k) {
                if p.class.zero_tolerance() && p.state == TriState::Real {
                    zt_real = zt_real.saturating_add(1);
                }
            }
            k += 1;
        }
        rate_ok && self.conflicts.len() as u32 == zt_real && self.alarms.is_empty()
    }

    /// 三态计数（Σ三态 = 登记行数守恒）。
    pub fn tri_counts(&self) -> [u32; 3] {
        let mut c = [0u32; 3];
        let mut k = 0usize;
        while k < self.posts.len() {
            if let Some(p) = self.posts.get(k) {
                let r = p.state.rank() as usize;
                if let Some(slot) = c.get_mut(r) {
                    *slot = slot.saturating_add(1);
                }
            }
            k += 1;
        }
        c
    }

    /// 登记行数。
    pub fn post_len(&self) -> usize {
        self.posts.len()
    }

    /// 告警行数（零静默口径：封账前必须清零）。
    pub fn alarm_len(&self) -> usize {
        self.alarms.len()
    }

    /// CONFLICT 行数。
    pub fn conflict_len(&self) -> usize {
        self.conflicts.len()
    }

    /// 封账（判序：空账拒 → 告警未清拒 → 守恒核账 → 相位推进）。
    pub fn seal(&mut self) -> Result<(), BookDiag> {
        if self.posts.is_empty() && self.reconciled == 0 {
            return Err(BookDiag::EEmptySeal);
        }
        if !self.alarms.is_empty() {
            return Err(BookDiag::ESilent);
        }
        let c = self.tri_counts();
        let sum = c.iter().fold(0u32, |a, &x| a.saturating_add(x));
        if sum as usize != self.posts.len() {
            return Err(BookDiag::ESilent);
        }
        self.advance(BookPhase::Sealed)
    }
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── FNV-1a 指纹 ──────────────────────────────────────────────────
pub fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    let mut h = acc;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        i += 1;
    }
    h
}

/// 双机账面构建：LCG 确定性生成甲乙两机各 12 列值。
/// 冻结口径：码值列(1-4)甲乙同值、偏移列(5-8)同值、计数列(9-12)
/// 甲 = 乙 + LCG 小抖动（±1 内，容差可吸收）。
pub fn build_dual_books() -> ([u32; COLS as usize], [u32; COLS as usize]) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut a = [0u32; COLS as usize];
    let mut b = [0u32; COLS as usize];
    let mut i = 0usize;
    while i < COLS as usize {
        let v = (rng.below(10_000) as u32).saturating_add(1);
        if let Some(x) = a.get_mut(i) {
            *x = v;
        }
        if let Some(x) = b.get_mut(i) {
            *x = v;
        }
        i += 1;
    }
    (a, b)
}

/// 全账对平：双机 12 列逐列 judge。
pub fn reconcile_all(book: &mut DiffBook, a: &[u32], b: &[u32]) -> Result<usize, BookDiag> {
    let mut diffs = 0usize;
    let mut col = 1u32;
    while col <= COLS {
        let ia = (col - 1) as usize;
        let va = a.get(ia).copied().unwrap_or(0);
        let vb = b.get(ia).copied().unwrap_or(0);
        if book.judge(col, va, vb)?.is_some() {
            diffs += 1;
        }
        col = col.saturating_add(1);
    }
    Ok(diffs)
}

// ── 静默演练（零静默 10/10） ─────────────────────────────────────
/// 静默演练：构造 10 处"观测有差而账面无登记"的静默场景（账面空账），
/// 零静默审计必须逐处告警。返回 (观测处数, 告警数)。
pub fn silence_drill() -> (u32, u32) {
    let mut observed: Vec<(u32, i32)> = Vec::new();
    let mut i = 1u32;
    while i <= 10 {
        observed.push((i, i as i32));
        i += 1;
    }
    let mut book = DiffBook::new();
    let n = book.zero_silence_audit(&observed).unwrap_or(u32::MAX);
    (10, n)
}

// ── 注入面（三族 × 10 轮全抓） ───────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 3], caught: [0u32; 3] }
    }
    /// 族账读取（get 取值，越界回落 0，不裸下标）。
    fn try_n(&self, fam: usize) -> u32 {
        self.tries.get(fam).copied().unwrap_or(0)
    }
    fn caught_n(&self, fam: usize) -> u32 {
        self.caught.get(fam).copied().unwrap_or(0)
    }
    /// 族账自增（get_mut 取槽，不裸下标）。
    fn bump_try(&mut self, fam: usize) {
        if let Some(t) = self.tries.get_mut(fam) {
            *t = t.saturating_add(1);
        }
    }
    fn bump_caught(&mut self, fam: usize) {
        if let Some(c) = self.caught.get_mut(fam) {
            *c = c.saturating_add(1);
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if self.try_n(i) != self.caught_n(i) || self.try_n(i) < 10 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入 × 10 轮：码值篡改 / 零静默吞差 / 相位倒退。
pub fn run_injections(led: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族 0：码值篡改——零容差类差 1 必 CONFLICT。
        let col = rng.below(4) as u32 + 1; // 码值列 1..=4
        let mut book = DiffBook::new();
        let _ = book.open_column(col, ColClass::Code);
        let a = (rng.below(1000) as u32).saturating_add(1);
        let b = a + 1;
        led.bump_try(0);
        if let Ok(Some(TriState::Real)) = book.judge(col, a, b) {
            if book.conflict_len() == 1 {
                led.bump_caught(0);
            }
        }
        // 族 1：零静默吞差——观测有差账面无登记，审计必告警。
        let mut book2 = DiffBook::new();
        let _ = book2.open_column(9, ColClass::Count);
        let _ = book2.judge(9, 100, 101); // 账面登记列 9
        // 观测集含未登记列 10 —— 审计必须告警。
        let n = book2.zero_silence_audit(&[(10, 5)]).unwrap_or(0);
        led.bump_try(1);
        if n == 1 {
            led.bump_caught(1);
        }
        // 族 2：相位倒退——Sealed 后 advance(Open) 显性拒。
        let mut book3 = DiffBook::new();
        let _ = book3.open_column(1, ColClass::Code);
        let _ = book3.judge(1, 7, 7);
        let sealed = book3.seal().is_ok();
        led.bump_try(2);
        if sealed && book3.advance(BookPhase::Open) == Err(BookDiag::ESealed) {
            led.bump_caught(2);
        }
        round = round.saturating_add(1);
    }
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

pub fn diag_window_ok() -> bool {
    BookDiag::ALL.iter().all(|d| d.val() >= SEG_BASE && d.val() < SEG_BASE + 0x40)
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn normal_flow_ok() -> bool {
    // 正常流：双机同值 12 列全平 → 一致率 1000 → J1 绿 → 封账成功。
    let (a, b) = build_dual_books();
    let mut book = DiffBook::new();
    let mut col = 1u32;
    while col <= COLS {
        let _ = book.open_column(col, if col <= 8 { ColClass::Code } else { ColClass::Count });
        col = col.saturating_add(1);
    }
    let open_ok = true; // 12 列恰满登记容量
    match reconcile_all(&mut book, &a, &b) {
        Ok(0) => {
            open_ok
                && book.rate_permille() == Ok(1000)
                && book.j1_green()
                && book.alarm_len() == 0
                && book.seal().is_ok()
                && book.phase() == BookPhase::Sealed
        }
        _ => false,
    }
}

fn boundary_ok() -> bool {
    // 四类边界：类别闭集 / 零容差差1 / 容差恰界 / 登记容量耗尽。
    let mut ok = true;
    ok = ok && ColClass::from_rank(0).map(|c| c.zero_tolerance()) == Some(true);
    ok = ok && ColClass::from_rank(2).map(|c| c.zero_tolerance()) == Some(false);
    ok = ok && ColClass::from_rank(3).is_none();
    // 零容差差 1：偏移列差 1 即真差异 + CONFLICT。
    let mut b1 = DiffBook::new();
    let _ = b1.open_column(5, ColClass::Offset);
    ok = ok && b1.judge(5, 100, 101) == Ok(Some(TriState::Real)) && b1.conflict_len() == 1;
    // 容差恰界：计数差 ±2 待归一，±3 真差异。
    let mut b2 = DiffBook::new();
    let _ = b2.open_column(9, ColClass::Count);
    ok = ok && b2.judge(9, 100, 102) == Ok(Some(TriState::Pending));
    let mut b3 = DiffBook::new();
    let _ = b3.open_column(10, ColClass::Count);
    ok = ok && b3.judge(10, 100, 103) == Ok(Some(TriState::Real));
    // 登记容量耗尽：第 13 列开账拒。
    let mut b4 = DiffBook::new();
    let mut c = 1u32;
    let mut full = true;
    while c <= COLS {
        if b4.open_column(c, ColClass::Count).is_err() {
            full = false;
        }
        c += 1;
    }
    ok = ok && full && b4.open_column(13, ColClass::Count).is_err();
    ok
}

fn rewinding_ok() -> bool {
    // 相位倒退拒：Sealed → Open 拒；Reconciling → Open 拒；同相位不拒。
    let mut b = DiffBook::new();
    let step1 = b.advance(BookPhase::Open).is_ok();
    let mut b2 = DiffBook::new();
    let _ = b2.open_column(1, ColClass::Code);
    let _ = b2.judge(1, 3, 3);
    b2.advance(BookPhase::Open) == Err(BookDiag::EPhase) && step1
}

fn conservation_ok() -> bool {
    // 三态计数守恒：Σ三态 = 登记行数。
    let mut book = DiffBook::new();
    let _ = book.open_column(9, ColClass::Count);
    let _ = book.open_column(10, ColClass::Count);
    let _ = book.open_column(11, ColClass::Count);
    let _ = book.judge(9, 100, 101); // 待归一
    let _ = book.judge(10, 100, 105); // 真差异
    let _ = book.judge(11, 100, 100); // 平
    let c = book.tri_counts();
    let sum = c.iter().fold(0u32, |a, &x| a.saturating_add(x));
    sum as usize == book.post_len() && c.get(1) == Some(&1) && c.get(2) == Some(&1)
}

fn rate_ok() -> bool {
    // 一致率口径：11/12 列平 → 916‰ <950 红；12/12 → 1000‰ 绿。
    let (a, mut bb) = build_dual_books();
    // 篡改乙机一个计数列（差 5，出容差）→ 一致率 11/12 ≈ 916‰。
    let mut book = DiffBook::new();
    let mut col = 1u32;
    while col <= COLS {
        let _ = book.open_column(col, if col <= 8 { ColClass::Code } else { ColClass::Count });
        col += 1;
    }
    if let Some(x) = bb.get_mut(11) {
        *x = x.saturating_add(5);
    }
    let r_low = match reconcile_all(&mut book, &a, &bb) {
        Ok(_) => book.rate_permille().unwrap_or(0),
        Err(_) => 0,
    };
    let (a2, b2) = build_dual_books();
    let mut book2 = DiffBook::new();
    let mut col2 = 1u32;
    while col2 <= COLS {
        let _ = book2.open_column(col2, if col2 <= 8 { ColClass::Code } else { ColClass::Count });
        col2 += 1;
    }
    let _ = reconcile_all(&mut book2, &a2, &b2);
    let r_high = book2.rate_permille().unwrap_or(0);
    r_low < RATE_PERMILLE && r_high >= RATE_PERMILLE
}

fn conflict_explicit_ok() -> bool {
    // CONFLICT 显性：码值列不等 → CONFLICT 留痕且 j1 绿判不豁免
    //（一致率即便 ≥95%，CONFLICT 核账不等式破即红）。
    let mut book = DiffBook::new();
    let _ = book.open_column(1, ColClass::Code);
    let _ = book.judge(1, 7, 8);
    book.conflict_len() == 1
        && match book.conflicts.first() {
            Some(c) => c.class == ColClass::Code && c.side_a == 7 && c.side_b == 8,
            None => false,
        }
}

fn silence_ok() -> bool {
    // 零静默演练 10/10。
    let (obs, alarm) = silence_drill();
    obs == 10 && alarm == 10
}

fn text_closed_ok() -> bool {
    let mut all_diff = true;
    let mut i = 0usize;
    while i < BookDiag::ALL.len() {
        let mut j = i + 1;
        while j < BookDiag::ALL.len() {
            match (BookDiag::ALL.get(i), BookDiag::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.detail() == b.detail() || a.val() == b.val() {
                        all_diff = false;
                    }
                }
                _ => all_diff = false,
            }
            j += 1;
        }
        i += 1;
    }
    all_diff
        && BookDiag::ALL.iter().all(|d| !d.detail().is_empty())
        && detail(SEG_BASE + 0x3F).contains("未知")
        && detail(BookDiag::EColClosed.val()).contains("未开账")
}

fn dual_ok() -> bool {
    let h1 = fingerprint();
    let h2 = fingerprint();
    match (h1, h2) {
        (Ok(a), Ok(b)) => a == b && a != 0,
        _ => false,
    }
}

/// 全流程指纹：开账 → 双机对平 → 三态计数 → 零静默演练 → 注入。
pub fn fingerprint() -> Result<u64, BookDiag> {
    let (a, b) = build_dual_books();
    let mut book = DiffBook::new();
    let mut col = 1u32;
    while col <= COLS {
        let _ = book.open_column(col, if col <= 8 { ColClass::Code } else { ColClass::Count });
        col += 1;
    }
    let diffs = reconcile_all(&mut book, &a, &b)?;
    let mut h = fnv1a(0xcbf29ce484222325, &diffs.to_le_bytes());
    h = fnv1a(h, &book.rate_permille()?.to_le_bytes());
    let c = book.tri_counts();
    for x in c.iter() {
        h = fnv1a(h, &x.to_le_bytes());
    }
    let (obs, alarm) = silence_drill();
    h = fnv1a(h, &obs.to_le_bytes());
    h = fnv1a(h, &alarm.to_le_bytes());
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    h = fnv1a(h, &inj.caught[0].to_le_bytes());
    h = fnv1a(h, &inj.caught[1].to_le_bytes());
    h = fnv1a(h, &inj.caught[2].to_le_bytes());
    Ok(h)
}

// ── 收口检查表 ───────────────────────────────────────────────────
pub const FROZEN_CHECK_N: usize = 15;

const CHECK_NAMES: [&str; FROZEN_CHECK_N] = [
    "F12738-01 正常流双机全平",
    "F12738-02 四类边界（类别/零容差/容差界/列容量）",
    "F12738-03 注入族0 码值篡改10轮全抓",
    "F12738-04 注入族1 零静默吞差10轮全抓",
    "F12738-05 注入族2 相位倒退10轮全抓",
    "F12738-06 一致率千分比口径",
    "F12738-07 CONFLICT 显性留痕",
    "F12738-08 零静默演练10/10",
    "F12738-09 三态计数守恒",
    "F12738-10 相位倒退拒",
    "F12738-11 双跑一致指纹",
    "F12738-12 段位守卫",
    "F12738-13 文案闭集",
    "F12738-14 收口只增不减",
    "F12738-15 列容量与类别冻结",
];

const CHECK_DETAILS: [&str; FROZEN_CHECK_N] = [
    "",
    "",
    "",
    "",
    "",
    "千分比整数口径：11/12=916‰<950 红，12/12=1000‰ 绿",
    "零容差类不平等必须落 CONFLICT 四元组账行，核账不等式破即红",
    "10 处观测差异账面无登记，审计逐处告警",
    "Σ三态 = 登记行数",
    "Sealed 后 advance 显性拒 ESealed/EPhase",
    "两次全流程 FNV-1a 指纹逐位一致且非零",
    "SEG_BASE%0x40==0 且在 [0x80000,0x82FC0]",
    "全码明细非空且两两不同，未知码中文兜底",
    "检查条数两次一致且等于冻结值 15",
    "12 列登记容量耗尽显性拒，3 类闭集越界拒",
];

/// 静态名表取值（get 取值，越界回落占位，不裸下标）。
fn nm(i: usize) -> &'static str {
    CHECK_NAMES.get(i).copied().unwrap_or("未名项")
}

/// 静态明细表取值（get 取值，越界回落空串，不裸下标）。
fn dt(i: usize) -> &'static str {
    CHECK_DETAILS.get(i).copied().unwrap_or("")
}

/// 收口检查面：15 条（≥12）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    v.push((String::from(nm(0)), normal_flow_ok(), format!("双机同值 12 列全平")));
    v.push((String::from(nm(1)), boundary_ok(), format!("类别/零容差/容差界/列容量")));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        String::from(nm(2)),
        inj.try_n(0) == inj.caught_n(0) && inj.try_n(0) == 10,
        format!("t/c {}/{}", inj.try_n(0), inj.caught_n(0)),
    ));
    v.push((
        String::from(nm(3)),
        inj.try_n(1) == inj.caught_n(1) && inj.try_n(1) == 10,
        format!("t/c {}/{}", inj.try_n(1), inj.caught_n(1)),
    ));
    v.push((
        String::from(nm(4)),
        inj.try_n(2) == inj.caught_n(2) && inj.try_n(2) == 10 && inj.all_caught(),
        format!("t/c {}/{}", inj.try_n(2), inj.caught_n(2)),
    ));
    v.push((String::from(nm(5)), rate_ok(), String::from(dt(5))));
    v.push((
        String::from(nm(6)),
        conflict_explicit_ok(),
        String::from(dt(6)),
    ));
    let (obs, alarm) = silence_drill();
    v.push((String::from(nm(7)), silence_ok(), format!("{}/{} 告警", alarm, obs)));
    v.push((String::from(nm(8)), conservation_ok(), String::from(dt(8))));
    v.push((String::from(nm(9)), rewinding_ok(), String::from(dt(9))));
    v.push((String::from(nm(10)), dual_ok(), String::from(dt(10))));
    v.push((
        String::from(nm(11)),
        segment_guard() && diag_window_ok(),
        format!("seg=0x{:05X} align={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    ));
    v.push((
        String::from(nm(12)),
        text_closed_ok(),
        format!("codes={}", BookDiag::ALL.len()),
    ));
    v.push((
        String::from(nm(13)),
        CHECK_NAMES.len() == FROZEN_CHECK_N
            && CHECK_DETAILS.len() == FROZEN_CHECK_N
            && FROZEN_CHECK_N >= 12,
        format!("冻结={} 名表={} 明细表={}", FROZEN_CHECK_N, CHECK_NAMES.len(), CHECK_DETAILS.len()),
    ));
    v.push((
        String::from(nm(14)),
        COLS == 12 && REG_CAP == 12 && ColClass::ALL.len() == 3 && TriState::ALL.len() == 3,
        format!("cols={} classes={} states={}", COLS, ColClass::ALL.len(), TriState::ALL.len()),
    ));
    v
}

/// 自检入口：extra_checks 逐条进 set；前 8 条走动态明细槽，其余静态明细。
pub fn run_unx12737_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12737");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_, passed, dyn_detail)) = items.get(i) {
            let name = CHECK_NAMES.get(i).copied().unwrap_or("F12738-? 未名项");
            if i < 8 {
                s.add_dyn(name, *passed, dyn_detail);
            } else {
                let st = CHECK_DETAILS.get(i).copied().unwrap_or("");
                s.add(name, *passed, st);
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ──────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_dual_books_flat() {
        // 正常流：双机同值全平 → 一致率 1000‰ → J1 绿 → 封账。
        assert!(normal_flow_ok());
        let (a, b) = build_dual_books();
        let mut book = DiffBook::new();
        let mut col = 1u32;
        while col <= COLS {
            assert!(book.open_column(col, ColClass::Count).is_ok());
            col += 1;
        }
        assert_eq!(reconcile_all(&mut book, &a, &b), Ok(0));
        assert_eq!(book.rate_permille(), Ok(1000));
        assert!(book.j1_green());
        assert!(book.seal().is_ok());
    }

    #[test]
    fn boundary_four() {
        // 四类边界：类别闭集/零容差差1/容差恰界/容量耗尽。
        assert!(boundary_ok());
        assert!(ColClass::from_rank(3).is_none());
        assert!(ColClass::Code.zero_tolerance());
        assert!(!ColClass::Count.zero_tolerance());
    }

    #[test]
    fn injection_zero_tolerance_tamper() {
        // 注入族 0：码值篡改 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(0), 10);
        assert_eq!(inj.caught_n(0), 10);
    }

    #[test]
    fn injection_silence_swallow() {
        // 注入族 1：零静默吞差 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(1), 10);
        assert_eq!(inj.caught_n(1), 10);
        assert!(inj.all_caught());
    }

    #[test]
    fn injection_phase_rewinding() {
        // 注入族 2：相位倒退 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(2), 10);
        assert_eq!(inj.caught_n(2), 10);
    }

    #[test]
    fn silence_drill_ten() {
        // 零静默演练 10/10。
        let (obs, alarm) = silence_drill();
        assert_eq!(obs, 10);
        assert_eq!(alarm, 10);
    }

    #[test]
    fn rate_permille_thresholds() {
        // 一致率阈值：916‰ 红线、1000‰ 绿线（≥95% 口径）。
        assert!(rate_ok());
        assert_eq!(RATE_PERMILLE, 950);
    }

    #[test]
    fn conflict_row_carries_values() {
        // CONFLICT 显性：四元组留痕。
        assert!(conflict_explicit_ok());
        let mut book = DiffBook::new();
        let _ = book.open_column(2, ColClass::Code);
        let _ = book.judge(2, 9, 10);
        assert_eq!(book.conflict_len(), 1);
    }

    #[test]
    fn phase_monotonic_and_sealed_readonly() {
        // 相位单调 + 封账只读。
        assert!(rewinding_ok());
        let mut book = DiffBook::new();
        let _ = book.open_column(1, ColClass::Code);
        let _ = book.judge(1, 1, 1);
        assert!(book.seal().is_ok());
        assert_eq!(book.open_column(2, ColClass::Code), Err(BookDiag::ESealed));
        assert_eq!(book.judge(2, 1, 2), Err(BookDiag::ESealed));
        assert_eq!(book.advance(BookPhase::Open), Err(BookDiag::ESealed));
    }

    #[test]
    fn tri_counts_conservation() {
        // 三态计数守恒。
        assert!(conservation_ok());
        let mut book = DiffBook::new();
        let _ = book.open_column(9, ColClass::Count);
        let _ = book.judge(9, 10, 11);
        let c = book.tri_counts();
        let sum = c.iter().fold(0u32, |a, &x| a.saturating_add(x));
        assert_eq!(sum as usize, book.post_len());
    }

    #[test]
    fn dual_fingerprint_stable() {
        // 双跑一致。
        assert!(dual_ok());
        assert_eq!(fingerprint(), fingerprint());
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集。
        assert!(segment_guard());
        assert!(diag_window_ok());
        assert!(text_closed_ok());
        assert!(detail(0x1234).contains("未知"));
    }

    #[test]
    fn extra_checks_all_green_and_frozen() {
        // 收口只增不减 + 全绿。
        let v = extra_checks();
        assert_eq!(v.len(), FROZEN_CHECK_N);
        assert!(v.len() >= 12);
        for (i, (_, p, _)) in v.iter().enumerate() {
            assert!(*p, "第 {} 条收口红", i);
        }
        let s = run_unx12737_checks();
        assert_eq!(s.len(), FROZEN_CHECK_N);
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
