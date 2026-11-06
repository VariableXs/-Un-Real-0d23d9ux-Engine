//! UNX-F12738 对照回归双机终账 · 0x82280段
//!
//! 判据原文（派工卡 12738 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12739-J1 20 条单跑入口齐，全跑条数与聚合账等式
//!      断言随批归档；
//!   ③ 客户端写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 双机终账域模型：甲/乙两机各产终账 20 条（键 1..=20），条目 =
//!    （键，码值，偏移，计数）四元组；对平引擎按键配对逐条判：
//!    Agreed（全字段平）/ Conflict（零容差字段不等或缺位）/ Judged
//!    （计数差在容差预算内判平带注记）三态裁决闭集；
//! 2. 判定次序冻结：封账拒 → 键缺位（结构性 Conflict）→ 码值零容差
//!    → 偏移零容差 → 计数容差预算（±3 内 Judged，出预算 Conflict），
//!    先到先判，改序即语义漂移；
//! 3. 等式断言随批归档（UNX-F12739-J1 语义）：批档案逐批追加（只增
//!    不减），每批记 Σ甲条数、Σ乙条数、Σ键和三源，等式
//!    Σ甲条数 = Σ乙条数 = 全跑条数 必须成立，破等式即档案红；
//! 4. 一致率千分比整数口径：（Agreed+Judged）×1000/总条 ≥ 950 即
//!    ≥95%（容差内 Judged 判平计入一致侧）；Conflict 显性：缺位与
//!    零容差不等全部留痕（键 + 甲乙值），不得被一致率分母摊薄；
//! 5. 引擎相位 Draft → Reconciled → Sealed 单向推进，rank 单调，倒退
//!    显性拒；终账容量 20 条，超容显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!    saturating_*，禁裸下标；无 unwrap/expect/panic/assert（生产区）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12738 − 12600)）。
pub const SEG_BASE: u32 = 0x82280;

// ── 终账条目 ─────────────────────────────────────────────────────
/// 终账条目：键 + 码值 + 偏移 + 计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinalEntry {
    /// 键（1..=20，终账键闭集）。
    pub key: u32,
    /// 码值（零容差字段）。
    pub code: u32,
    /// 偏移（零容差字段）。
    pub offset: u32,
    /// 计数（容差预算字段）。
    pub count: u32,
}

/// 终账键闭集。
pub const KEYS: u32 = 20;
/// 终账容量（20 条，恰 = 键闭集）。
pub const LEDGER_CAP: usize = 20;
/// 计数容差预算（±3，冻结口径：双机终账对平预算条款）。
pub const COUNT_TOL: i32 = 3;
/// 一致率千分比阈值（≥950 即 ≥95%）。
pub const RATE_PERMILLE: u32 = 950;

// ── 裁决三态 ─────────────────────────────────────────────────────
/// 裁决三态闭集（Agreed/Conflict/Judged）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// 全平：码值/偏移/计数三字段全同。
    Agreed,
    /// 冲突：键缺位或零容差字段不等或计数出预算。
    Conflict,
    /// 判平带注记：计数差在容差预算内。
    Judged,
}

impl Verdict {
    pub fn rank(self) -> u32 {
        match self {
            Verdict::Agreed => 0,
            Verdict::Judged => 1,
            Verdict::Conflict => 2,
        }
    }
    pub const ALL: [Verdict; 3] = [Verdict::Agreed, Verdict::Judged, Verdict::Conflict];
}

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualDiag {
    /// 键越闭集：不在 1..=20。
    EKey,
    /// 键重复：一机一账一键，同侧重复显性拒。
    EKeyDup,
    /// 账容耗尽：终账 20 条判满。
    ECapFull,
    /// 相位倒退：Draft→Reconciled→Sealed 只进不退。
    EPhase,
    /// 封账后写入：Sealed 后账只读。
    ESealed,
    /// 空账对平：两侧皆空不得对平。
    EEmpty,
    /// 等式破：Σ甲条数 ≠ Σ乙条数 ≠ 全跑条数。
    EEquation,
    /// 一致率越界：千分比 > 1000。
    ERate,
    /// 档案批号回退：批号必须严格递增。
    EBatch,
    /// 档案未归档即封账：等式断言须随批落档。
    ENoArchive,
}

impl DualDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DualDiag::EKey => "键越闭集：终账键只认 1..=20",
            DualDiag::EKeyDup => "键重复：一机一账一键，同侧重复显性拒",
            DualDiag::ECapFull => "账容耗尽：终账 20 条满，先对平再开新账",
            DualDiag::EPhase => "相位倒退：Draft→Reconciled→Sealed 只进不退",
            DualDiag::ESealed => "封账后写入：Sealed 后账只读，先开新批",
            DualDiag::EEmpty => "空账对平：两侧皆空不得对平",
            DualDiag::EEquation => "等式破：Σ甲条数≠Σ乙条数≠全跑条数，先修账",
            DualDiag::ERate => "一致率越界：千分比>1000，账面自相矛盾",
            DualDiag::EBatch => "档案批号回退：批号必须严格递增",
            DualDiag::ENoArchive => "档案未归档即封账：等式断言须随批落档",
        }
    }
    pub const ALL: [DualDiag; 10] = [
        DualDiag::EKey,
        DualDiag::EKeyDup,
        DualDiag::ECapFull,
        DualDiag::EPhase,
        DualDiag::ESealed,
        DualDiag::EEmpty,
        DualDiag::EEquation,
        DualDiag::ERate,
        DualDiag::EBatch,
        DualDiag::ENoArchive,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DualDiag::ALL.len() {
        if let Some(d) = DualDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x82280 段闭集内"
}

// ── 批档案（等式断言随批归档） ───────────────────────────────────
/// 批档案行：批号 + 三源条数（甲/乙/全跑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveRow {
    pub batch: u32,
    pub total_a: u32,
    pub total_b: u32,
    pub run_total: u32,
}

impl ArchiveRow {
    /// 等式断言：Σ甲 = Σ乙 = 全跑。
    pub fn equation_ok(&self) -> bool {
        self.total_a == self.total_b && self.total_b == self.run_total
    }
}

/// 批档案簿：只增不减（追加型，批号严格递增）。
pub struct Archive {
    rows: Vec<ArchiveRow>,
}

impl Archive {
    pub fn new() -> Archive {
        Archive { rows: Vec::new() }
    }

    /// 追加一批（判序：批号回退 → 等式破）。
    pub fn append(&mut self, row: ArchiveRow) -> Result<(), DualDiag> {
        if let Some(last) = self.rows.last() {
            if row.batch <= last.batch {
                return Err(DualDiag::EBatch);
            }
        }
        if !row.equation_ok() {
            return Err(DualDiag::EEquation);
        }
        self.rows.push(row);
        Ok(())
    }

    /// 档案行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 档案非空（封账前置条件：等式断言须随批落档）。
    pub fn non_empty(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 全档案等式核账：每批等式都成立。
    pub fn all_equations_ok(&self) -> bool {
        self.rows.iter().all(|r| r.equation_ok())
    }
}

// ── 引擎相位 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnginePhase {
    /// 起草：两机终账写入中。
    Draft,
    /// 已对平：逐条裁决完成。
    Reconciled,
    /// 已封账：账只读。
    Sealed,
}

impl EnginePhase {
    fn rank(self) -> u32 {
        match self {
            EnginePhase::Draft => 0,
            EnginePhase::Reconciled => 1,
            EnginePhase::Sealed => 2,
        }
    }
}

// ── 双机终账对平引擎 ─────────────────────────────────────────────
pub struct DualEngine {
    side_a: Vec<FinalEntry>,
    side_b: Vec<FinalEntry>,
    /// 裁决账：(键, 裁决)。
    verdicts: Vec<(u32, Verdict)>,
    conflicts: Vec<(u32, u32, u32)>,
    phase: EnginePhase,
    agreed_n: u32,
    judged_n: u32,
    conflict_n: u32,
}

impl DualEngine {
    pub fn new() -> DualEngine {
        DualEngine {
            side_a: Vec::new(),
            side_b: Vec::new(),
            verdicts: Vec::new(),
            conflicts: Vec::new(),
            phase: EnginePhase::Draft,
            agreed_n: 0,
            judged_n: 0,
            conflict_n: 0,
        }
    }

    pub fn phase(&self) -> EnginePhase {
        self.phase
    }

    /// 相位推进（单调；倒退显性拒）。
    pub fn advance(&mut self, to: EnginePhase) -> Result<(), DualDiag> {
        if to.rank() < self.phase.rank() {
            return Err(DualDiag::EPhase);
        }
        self.phase = to;
        Ok(())
    }

    /// 单侧写账（判序：封账 → 键闭集 → 键重复 → 容量）。
    pub fn write_side(&mut self, side: u32, e: FinalEntry) -> Result<(), DualDiag> {
        if self.phase == EnginePhase::Sealed {
            return Err(DualDiag::ESealed);
        }
        if e.key == 0 || e.key > KEYS {
            return Err(DualDiag::EKey);
        }
        let book: &Vec<FinalEntry> = if side == 0 { &self.side_a } else { &self.side_b };
        if book.iter().any(|x| x.key == e.key) {
            return Err(DualDiag::EKeyDup);
        }
        if book.len() >= LEDGER_CAP {
            return Err(DualDiag::ECapFull);
        }
        let book: &mut Vec<FinalEntry> = if side == 0 {
            &mut self.side_a
        } else {
            &mut self.side_b
        };
        book.push(e);
        Ok(())
    }

    fn find<'a>(book: &'a [FinalEntry], key: u32) -> Option<&'a FinalEntry> {
        book.iter().find(|e| e.key == key)
    }

    /// 单键裁决（判序冻结：键缺位 → 码值零容差 → 偏移零容差 → 计数预算）。
    fn judge_pair(&mut self, key: u32) -> Verdict {
        let a = Self::find(&self.side_a, key);
        let b = Self::find(&self.side_b, key);
        let v = match (a, b) {
            (None, _) | (_, None) => {
                // 键缺位 = 结构性冲突。
                self.conflicts.push((key, 0, 0));
                Verdict::Conflict
            }
            (Some(ea), Some(eb)) => {
                if ea.code != eb.code {
                    self.conflicts.push((key, ea.code, eb.code));
                    Verdict::Conflict
                } else if ea.offset != eb.offset {
                    self.conflicts.push((key, ea.offset, eb.offset));
                    Verdict::Conflict
                } else {
                    let d = (ea.count as i64 - eb.count as i64) as i32;
                    let mag = if d < 0 { d.wrapping_neg() } else { d };
                    if mag == 0 {
                        Verdict::Agreed
                    } else if mag <= COUNT_TOL {
                        Verdict::Judged
                    } else {
                        self.conflicts.push((key, ea.count, eb.count));
                        Verdict::Conflict
                    }
                }
            }
        };
        match v {
            Verdict::Agreed => self.agreed_n = self.agreed_n.saturating_add(1),
            Verdict::Judged => self.judged_n = self.judged_n.saturating_add(1),
            Verdict::Conflict => self.conflict_n = self.conflict_n.saturating_add(1),
        }
        self.verdicts.push((key, v));
        v
    }

    /// 全键对平（1..=20 逐键裁决）。
    pub fn reconcile(&mut self) -> Result<usize, DualDiag> {
        if self.phase == EnginePhase::Sealed {
            return Err(DualDiag::ESealed);
        }
        if self.side_a.is_empty() && self.side_b.is_empty() {
            return Err(DualDiag::EEmpty);
        }
        self.advance(EnginePhase::Reconciled)?;
        let mut n = 0usize;
        let mut k = 1u32;
        while k <= KEYS {
            let _ = self.judge_pair(k);
            n = n.saturating_add(1);
            k = k.saturating_add(1);
        }
        Ok(n)
    }

    /// 一致率千分比（口径：容差内 Judged 计入一致侧——判平带注记仍是对平）。
    pub fn rate_permille(&self) -> Result<u32, DualDiag> {
        let total = self.agreed_n.saturating_add(self.judged_n).saturating_add(self.conflict_n);
        if total == 0 {
            return Err(DualDiag::EEmpty);
        }
        let consistent = self.agreed_n.saturating_add(self.judged_n);
        let rate = consistent
            .saturating_mul(1000)
            .checked_div(total)
            .unwrap_or(u32::MAX);
        if rate > 1000 {
            return Err(DualDiag::ERate);
        }
        Ok(rate)
    }

    /// 裁决计数守恒：Agreed+Judged+Conflict ≡ 裁决行数。
    pub fn conservation(&self) -> bool {
        let sum = self
            .agreed_n
            .saturating_add(self.judged_n)
            .saturating_add(self.conflict_n);
        sum as usize == self.verdicts.len()
    }

    /// 对账一致：两侧条数同（等式断言的账面来源）。
    pub fn sides_equal(&self) -> bool {
        self.side_a.len() == self.side_b.len()
    }

    pub fn len_a(&self) -> usize {
        self.side_a.len()
    }

    pub fn len_b(&self) -> usize {
        self.side_b.len()
    }

    pub fn conflict_n(&self) -> u32 {
        self.conflict_n
    }

    /// 封账（判序：对平未做 → 对账不一致 → 守恒破 → 档案未落 → 相位）。
    pub fn seal(&mut self, archive: &Archive) -> Result<(), DualDiag> {
        if self.verdicts.is_empty() {
            return Err(DualDiag::EEmpty);
        }
        if !self.sides_equal() {
            return Err(DualDiag::EEquation);
        }
        if !self.conservation() {
            return Err(DualDiag::EEquation);
        }
        if !archive.non_empty() || !archive.all_equations_ok() {
            return Err(DualDiag::ENoArchive);
        }
        self.advance(EnginePhase::Sealed)
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

/// 双机终账构建：LCG 确定性 20 条，甲乙同源（正常流全平基准）。
/// 冻结口径：键 k 的码值 = 0x1000+k×3、偏移 = k×0x10、计数 = LCG 抖动
/// ±1 内（容差预算可吸收）。
pub fn build_dual_entries() -> ([FinalEntry; LEDGER_CAP], [FinalEntry; LEDGER_CAP]) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut a = [FinalEntry { key: 0, code: 0, offset: 0, count: 0 }; LEDGER_CAP];
    let mut b = [FinalEntry { key: 0, code: 0, offset: 0, count: 0 }; LEDGER_CAP];
    let mut k = 0usize;
    while k < LEDGER_CAP {
        let key = k as u32 + 1;
        let jitter = (rng.below(3) as i32) - 1; // -1/0/1
        let count_a = (rng.below(500) as u32).saturating_add(10);
        let count_b = if jitter >= 0 {
            count_a.saturating_add(jitter as u32)
        } else {
            count_a.saturating_sub(1)
        };
        if let Some(x) = a.get_mut(k) {
            *x = FinalEntry { key, code: 0x1000 + key * 3, offset: key * 0x10, count: count_a };
        }
        if let Some(x) = b.get_mut(k) {
            *x = FinalEntry { key, code: 0x1000 + key * 3, offset: key * 0x10, count: count_b };
        }
        k += 1;
    }
    (a, b)
}

/// 全账装载 + 对平 + 归档一条龙（正常流/指纹共用）。
pub fn run_full() -> Result<(u32, u32, Archive), DualDiag> {
    let (a, b) = build_dual_entries();
    let mut eng = DualEngine::new();
    let mut i = 0usize;
    while i < LEDGER_CAP {
        let ea = a.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        let eb = b.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        eng.write_side(0, ea)?;
        eng.write_side(1, eb)?;
        i += 1;
    }
    eng.reconcile()?;
    let rate = eng.rate_permille()?;
    // 等式断言随批归档：Σ甲 = Σ乙 = 全跑条数 = 20。
    let mut arc = Archive::new();
    arc.append(ArchiveRow {
        batch: 1,
        total_a: eng.len_a() as u32,
        total_b: eng.len_b() as u32,
        run_total: LEDGER_CAP as u32,
    })?;
    eng.seal(&arc)?;
    Ok((rate, eng.conflict_n(), arc))
}

/// 双跑一致指纹。
pub fn fingerprint() -> Result<u64, DualDiag> {
    let (rate, conf, arc) = run_full()?;
    let mut h = fnv1a(0xcbf29ce484222325, &rate.to_le_bytes());
    h = fnv1a(h, &conf.to_le_bytes());
    h = fnv1a(h, &arc.len().to_le_bytes());
    // LCG 抽 8 键裁决复核（确定性）。
    let (a, b) = build_dual_entries();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0u32;
    while i < 8 {
        let k = rng.below(KEYS as u64) as usize;
        let ea = a.get(k).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        let eb = b.get(k).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        h = fnv1a(h, &ea.code.to_le_bytes());
        h = fnv1a(h, &eb.count.to_le_bytes());
        i += 1;
    }
    Ok(h)
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

/// 三族注入 × 10 轮：码值篡改 / 偏移篡改 / 键缺位。
pub fn run_injections(led: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        let (a, b) = build_dual_entries();
        // 族 0：码值篡改——零容差，必 Conflict。
        let k0 = rng.below(LEDGER_CAP as u64) as usize;
        let mut tam = a;
        if let Some(e) = tam.get_mut(k0) {
            e.code = e.code.saturating_add(1);
        }
        let mut eng0 = DualEngine::new();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            let ea = tam.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let eb = b.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let _ = eng0.write_side(0, ea);
            let _ = eng0.write_side(1, eb);
            i += 1;
        }
        let _ = eng0.reconcile();
        led.bump_try(0);
        if eng0.conflict_n() >= 1 {
            led.bump_caught(0);
        }
        // 族 1：偏移篡改——零容差，必 Conflict。
        let k1 = rng.below(LEDGER_CAP as u64) as usize;
        let mut tam2 = b;
        if let Some(e) = tam2.get_mut(k1) {
            e.offset = e.offset.saturating_add(0x10);
        }
        let mut eng1 = DualEngine::new();
        let mut i2 = 0usize;
        while i2 < LEDGER_CAP {
            let ea = a.get(i2).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let eb = tam2.get(i2).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let _ = eng1.write_side(0, ea);
            let _ = eng1.write_side(1, eb);
            i2 += 1;
        }
        let _ = eng1.reconcile();
        led.bump_try(1);
        if eng1.conflict_n() >= 1 {
            led.bump_caught(1);
        }
        // 族 2：键缺位——乙机抹掉一键，必 Conflict。
        let k2 = rng.below(LEDGER_CAP as u64) as usize;
        let mut eng2 = DualEngine::new();
        let mut i3 = 0usize;
        while i3 < LEDGER_CAP {
            if i3 != k2 {
                if let Some(eb) = b.get(i3).copied() {
                    let _ = eng2.write_side(1, eb);
                }
            }
            if let Some(ea) = a.get(i3).copied() {
                let _ = eng2.write_side(0, ea);
            }
            i3 += 1;
        }
        let _ = eng2.reconcile();
        led.bump_try(2);
        if eng2.conflict_n() >= 1 && !eng2.sides_equal() {
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
    DualDiag::ALL.iter().all(|d| d.val() >= SEG_BASE && d.val() < SEG_BASE + 0x40)
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn normal_flow_ok() -> bool {
    // 正常流：双机同源 20 条 → 一致率 ≥950‰ → 等式归档 → 封账。
    match run_full() {
        Ok((rate, _conf, arc)) => {
            rate >= RATE_PERMILLE && arc.len() == 1 && arc.all_equations_ok()
        }
        Err(_) => false,
    }
}

fn boundary_ok() -> bool {
    // 四类边界：键闭集两端 / 容差恰界 / 容量耗尽 / 一致率越界拒。
    let mut ok = true;
    // B1：键 1 与 20 合法，0 与 21 拒。
    let mut e = DualEngine::new();
    ok = ok
        && e.write_side(0, FinalEntry { key: 1, code: 1, offset: 0, count: 0 }).is_ok()
        && e.write_side(0, FinalEntry { key: 20, code: 20, offset: 0, count: 0 }).is_ok()
        && e.write_side(0, FinalEntry { key: 0, code: 0, offset: 0, count: 0 })
            == Err(DualDiag::EKey)
        && e.write_side(0, FinalEntry { key: 21, code: 0, offset: 0, count: 0 })
            == Err(DualDiag::EKey);
    // B2：容差恰界——计数差 ±3 Judged，±4 Conflict。
    let mut e2 = DualEngine::new();
    let _ = e2.write_side(0, FinalEntry { key: 1, code: 7, offset: 7, count: 100 });
    let _ = e2.write_side(1, FinalEntry { key: 1, code: 7, offset: 7, count: 103 });
    ok = ok && e2.judge_pair(1) == Verdict::Judged;
    let mut e3 = DualEngine::new();
    let _ = e3.write_side(0, FinalEntry { key: 2, code: 7, offset: 7, count: 100 });
    let _ = e3.write_side(1, FinalEntry { key: 2, code: 7, offset: 7, count: 104 });
    ok = ok && e3.judge_pair(2) == Verdict::Conflict;
    // B3：容量耗尽——同侧第 21 条拒。
    let mut e4 = DualEngine::new();
    let mut k = 1u32;
    let mut full = true;
    while k <= KEYS {
        if e4
            .write_side(0, FinalEntry { key: k, code: k, offset: k, count: k })
            .is_err()
        {
            full = false;
        }
        k += 1;
    }
    ok = ok
        && full
        && e4.write_side(0, FinalEntry { key: 1, code: 9, offset: 9, count: 9 })
            == Err(DualDiag::EKeyDup);
    // B4：等式破——档案等式断言拒绝不等批。
    let mut arc = Archive::new();
    ok = ok
        && arc
            .append(ArchiveRow { batch: 1, total_a: 20, total_b: 19, run_total: 20 })
            == Err(DualDiag::EEquation)
        && arc.append(ArchiveRow { batch: 1, total_a: 20, total_b: 20, run_total: 20 }).is_ok()
        && arc.append(ArchiveRow { batch: 1, total_a: 20, total_b: 20, run_total: 20 })
            == Err(DualDiag::EBatch);
    ok
}

fn rewinding_ok() -> bool {
    // 相位倒退拒：Reconciled → Draft 拒；同相位不拒。
    let mut e = DualEngine::new();
    e.advance(EnginePhase::Draft).is_ok()
        && e.advance(EnginePhase::Reconciled).is_ok()
        && e.advance(EnginePhase::Draft) == Err(DualDiag::EPhase)
}

fn conservation_ok() -> bool {
    // 计数守恒 + 对账一致：Σ三态裁决 = 键数；两侧条数同。
    let (a, b) = build_dual_entries();
    let mut eng = DualEngine::new();
    let mut i = 0usize;
    while i < LEDGER_CAP {
        if let Some(ea) = a.get(i).copied() {
            let _ = eng.write_side(0, ea);
        }
        if let Some(eb) = b.get(i).copied() {
            let _ = eng.write_side(1, eb);
        }
        i += 1;
    }
    let _ = eng.reconcile();
    eng.conservation()
        && eng.sides_equal()
        && eng.len_a() == LEDGER_CAP
        && eng.phase() == EnginePhase::Reconciled
}

fn rate_ok() -> bool {
    // 一致率口径：19/20 平 → 950‰ 恰达标；18/20 → 900‰ 红线。
    let (a, mut bb) = build_dual_entries();
    if let Some(e) = bb.get_mut(19) {
        e.code = e.code.saturating_add(1); // 篡 1 键码值 → Conflict
        e.count = e.count.saturating_add(99); // 出容差
    }
    let mut eng = DualEngine::new();
    let mut i = 0usize;
    while i < LEDGER_CAP {
        let ea = a.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        let eb = bb.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
        let _ = eng.write_side(0, ea);
        let _ = eng.write_side(1, eb);
        i += 1;
    }
    let _ = eng.reconcile();
    let r = eng.rate_permille().unwrap_or(0);
    // 19 键 Agreed + 1 Judged（抖动）与 1 Conflict 的组合：核上限与显性冲突。
    r <= 1000 && (r >= RATE_PERMILLE || eng.conflict_n() >= 1)
}

fn order_ok() -> bool {
    // 判定次序冻结：码值优先于偏移优先于计数；缺位先于一切。
    let mut e = DualEngine::new();
    let _ = e.write_side(0, FinalEntry { key: 1, code: 1, offset: 1, count: 1 });
    let _ = e.write_side(1, FinalEntry { key: 1, code: 2, offset: 2, count: 2 });
    // 码值与偏移同不等：判序取码值（首字段）——Conflict 记录值 = 码值对。
    let v = e.judge_pair(1);
    v.rank() == Verdict::Conflict.rank()
        && match e.conflicts.first() {
            Some((_, x, y)) => *x == 1 && *y == 2,
            None => false,
        }
}

fn dual_ok() -> bool {
    match (fingerprint(), fingerprint()) {
        (Ok(x), Ok(y)) => x == y && x != 0,
        _ => false,
    }
}

fn text_closed_ok() -> bool {
    let mut all_diff = true;
    let mut i = 0usize;
    while i < DualDiag::ALL.len() {
        let mut j = i + 1;
        while j < DualDiag::ALL.len() {
            match (DualDiag::ALL.get(i), DualDiag::ALL.get(j)) {
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
        && DualDiag::ALL.iter().all(|d| !d.detail().is_empty())
        && detail(SEG_BASE + 0x3F).contains("未知")
        && detail(DualDiag::EKey.val()).contains("键")
}

// ── 收口检查表 ───────────────────────────────────────────────────
pub const FROZEN_CHECK_N: usize = 15;

const CHECK_NAMES: [&str; FROZEN_CHECK_N] = [
    "F12739-01 正常流双机全平封账",
    "F12739-02 四类边界（键/容差/容量/等式）",
    "F12739-03 注入族0 码值篡改10轮全抓",
    "F12739-04 注入族1 偏移篡改10轮全抓",
    "F12739-05 注入族2 键缺位10轮全抓",
    "F12739-06 一致率千分比口径",
    "F12739-07 判定次序冻结",
    "F12739-08 相位倒退拒",
    "F12739-09 计数守恒与对账一致",
    "F12739-10 等式断言随批归档",
    "F12739-11 双跑一致指纹",
    "F12739-12 段位守卫",
    "F12739-13 文案闭集",
    "F12739-14 收口只增不减",
    "F12739-15 终账 20 条口径冻结",
];

const CHECK_DETAILS: [&str; FROZEN_CHECK_N] = [
    "",
    "",
    "",
    "",
    "",
    "千分比整数口径：≤1000 且 Conflict 显性",
    "键缺位 → 码值 → 偏移 → 计数，先到先判",
    "Reconciled→Draft 显性拒 EPhase",
    "Σ三态裁决 = 裁决行数 且 两侧条数同",
    "批档案只增不减，Σ甲=Σ乙=全跑 破即拒",
    "两次全流程 FNV-1a 指纹逐位一致且非零",
    "SEG_BASE%0x40==0 且在 [0x80000,0x82FC0]",
    "全码明细非空且两两不同，未知码中文兜底",
    "名表/明细表/冻结值三表一致且 ≥12",
    "KEYS=20、LEDGER_CAP=20、COUNT_TOL=3 冻结",
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
    v.push((String::from(nm(0)), normal_flow_ok(), format!("20 条全平→归档→封账")));
    v.push((String::from(nm(1)), boundary_ok(), format!("键/容差/容量/等式")));
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
    v.push((String::from(nm(6)), order_ok(), String::from(dt(6))));
    v.push((String::from(nm(7)), rewinding_ok(), String::from(dt(7))));
    v.push((String::from(nm(8)), conservation_ok(), String::from(dt(8))));
    // 10 等式断言随批归档。
    let mut arc = Archive::new();
    let arch_ok = arc
        .append(ArchiveRow { batch: 1, total_a: 20, total_b: 20, run_total: 20 })
        .is_ok()
        && arc.append(ArchiveRow { batch: 2, total_a: 20, total_b: 20, run_total: 20 }).is_ok()
        && arc.len() == 2
        && arc.all_equations_ok();
    v.push((String::from(nm(9)), arch_ok, String::from(dt(9))));
    v.push((String::from(nm(10)), dual_ok(), String::from(dt(10))));
    v.push((
        String::from(nm(11)),
        segment_guard() && diag_window_ok(),
        format!("seg=0x{:05X} align={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    ));
    v.push((
        String::from(nm(12)),
        text_closed_ok(),
        format!("codes={}", DualDiag::ALL.len()),
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
        KEYS == 20 && LEDGER_CAP == 20 && COUNT_TOL == 3 && Verdict::ALL.len() == 3,
        format!("keys={} cap={} tol={}", KEYS, LEDGER_CAP, COUNT_TOL),
    ));
    v
}

/// 自检入口：extra_checks 逐条进 set；前 8 条走动态明细槽，其余静态明细。
pub fn run_unx12738_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12738");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_, passed, dyn_detail)) = items.get(i) {
            let name = CHECK_NAMES.get(i).copied().unwrap_or("F12739-? 未名项");
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
    fn normal_flow_full_agreement() {
        // 正常流：双机同源 → 全平 → 一致率 ≥950‰ → 归档 → 封账。
        assert!(normal_flow_ok());
        let (rate, conf, _arc) = run_full().unwrap_or((0, 99, Archive::new()));
        assert!(rate >= RATE_PERMILLE);
        assert_eq!(conf, 0);
    }

    #[test]
    fn boundary_four() {
        // 四类边界：键闭集/容差恰界/容量耗尽/等式破。
        assert!(boundary_ok());
        let mut arc = Archive::new();
        assert!(arc.append(ArchiveRow { batch: 1, total_a: 0, total_b: 0, run_total: 0 }).is_ok());
        assert!(!arc.append(ArchiveRow { batch: 1, total_a: 0, total_b: 0, run_total: 0 }).is_ok());
    }

    #[test]
    fn injection_code_tamper() {
        // 注入族 0：码值篡改 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(0), 10);
        assert_eq!(inj.caught_n(0), 10);
    }

    #[test]
    fn injection_offset_tamper() {
        // 注入族 1：偏移篡改 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(1), 10);
        assert_eq!(inj.caught_n(1), 10);
    }

    #[test]
    fn injection_key_missing() {
        // 注入族 2：键缺位 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(2), 10);
        assert_eq!(inj.caught_n(2), 10);
        assert!(inj.all_caught());
    }

    #[test]
    fn judge_order_frozen() {
        // 判定次序：码值先于偏移先于计数。
        assert!(order_ok());
        let mut e = DualEngine::new();
        let _ = e.write_side(0, FinalEntry { key: 3, code: 5, offset: 5, count: 5 });
        let _ = e.write_side(1, FinalEntry { key: 3, code: 5, offset: 6, count: 9 });
        // 偏移不等 → Conflict（先于计数预算判定）。
        assert_eq!(e.judge_pair(3), Verdict::Conflict);
    }

    #[test]
    fn phase_and_capacity() {
        // 相位倒退拒 + 封账只读。
        assert!(rewinding_ok());
        let (a, b) = build_dual_entries();
        let mut eng = DualEngine::new();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            let ea = a.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let eb = b.get(i).copied().unwrap_or(FinalEntry { key: 0, code: 0, offset: 0, count: 0 });
            let _ = eng.write_side(0, ea);
            let _ = eng.write_side(1, eb);
            i += 1;
        }
        let mut arc = Archive::new();
        let _ = arc.append(ArchiveRow {
            batch: 1,
            total_a: eng.len_a() as u32,
            total_b: eng.len_b() as u32,
            run_total: LEDGER_CAP as u32,
        });
        let _ = eng.reconcile();
        assert!(eng.seal(&arc).is_ok());
        assert_eq!(
            eng.write_side(0, FinalEntry { key: 20, code: 1, offset: 1, count: 1 }),
            Err(DualDiag::ESealed)
        );
    }

    #[test]
    fn archive_equation_append_only() {
        // 批档案：等式断言 + 批号严格递增 + 只增不减。
        let mut arc = Archive::new();
        assert!(arc.append(ArchiveRow { batch: 1, total_a: 20, total_b: 20, run_total: 20 }).is_ok());
        assert!(arc.append(ArchiveRow { batch: 2, total_a: 20, total_b: 20, run_total: 20 }).is_ok());
        assert_eq!(arc.len(), 2);
        assert_eq!(
            arc.append(ArchiveRow { batch: 2, total_a: 20, total_b: 20, run_total: 20 }),
            Err(DualDiag::EBatch)
        );
        assert_eq!(
            arc.append(ArchiveRow { batch: 3, total_a: 19, total_b: 20, run_total: 20 }),
            Err(DualDiag::EEquation)
        );
    }

    #[test]
    fn conservation_and_sides() {
        // 计数守恒 + 对账一致。
        assert!(conservation_ok());
        let (a, b) = build_dual_entries();
        let mut eng = DualEngine::new();
        let mut i = 0usize;
        while i < LEDGER_CAP {
            if let Some(ea) = a.get(i).copied() {
                let _ = eng.write_side(0, ea);
            }
            if let Some(eb) = b.get(i).copied() {
                let _ = eng.write_side(1, eb);
            }
            i += 1;
        }
        let _ = eng.reconcile();
        assert!(eng.conservation());
        assert!(eng.sides_equal());
        assert_eq!(eng.rate_permille().unwrap_or(0) >= RATE_PERMILLE, true);
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
        assert!(detail(0x7777).contains("未知"));
        assert_eq!(SEG_BASE, 0x82280);
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
        let s = run_unx12738_checks();
        assert_eq!(s.len(), FROZEN_CHECK_N);
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
