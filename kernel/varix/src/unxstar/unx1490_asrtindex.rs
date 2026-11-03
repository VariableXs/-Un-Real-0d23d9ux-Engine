//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1490 · 域级断言索引 v1（全域唯一键与查询面） · 0x75D40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批5-K）：
//!   全域唯一键（判据号+断言号复合键），唯一性常绿，查询面（按判据
//!   号/按段位/按状态三维度）与账本对拍一致率 100%，越界查询显性
//!   Empty
//! 设计要点：
//!   1. 复合键冻结：判据号 4 位（1000~9999）+ 断言号 3 位（0~999），
//!      复合值 = 判据号×1000 + 断言号，键格式闸先于一切登记；
//!   2. 唯一性常绿：复合键全域查重，重复登记显性拒（DuplicateKey）
//!      并入拒账，登记 N 枚后全域 distinct 数恒等账面条数；
//!   3. 状态闭集单调：绿/红/封三态，只许前进（绿→红→封），倒退显
//!      性拒（StatusRegress），无还原门；
//!   4. 查询面三维度：按判据号 / 按段位 / 按状态，逐维度与账本线性
//!      扫描独立对拍，一致率 100%；
//!   5. 越界查询显性：键格式非法（判据号非 4 位等）显性拒
//!      （KeyRange）；格式合法但无命中显性回空（Ok 空表，不静默）；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75D40，AI-VX7-1104 批5-K 独占槽）。
pub const SEG_BASE: u32 = 0x75D40;

// ── 冻结口径 ────────────────────────────────────────────────────
/// 判据号下界（4 位：1000）。
pub const CRIT_MIN: u16 = 1000;
/// 判据号上界（4 位：9999）。
pub const CRIT_MAX: u16 = 9999;
/// 断言号上界（3 位：0~999）。
pub const ASSERT_MAX: u16 = 999;
/// 复合键倍率（判据号×1000 + 断言号）。
pub const KEY_SCALE: u32 = 1000;
/// 语料条数（冻结 12）。
pub const CORPUS_N: usize = 12;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdxCode {
    /// 键格式越界（判据号非 4 位或断言号非 3 位）
    KeyRange,
    /// 复合键重复（全域唯一性破坏）
    DuplicateKey,
    /// 状态倒退（单调闸显性拒）
    StatusRegress,
    /// 键不在账（置状态/查无此键）
    UnknownKey,
    /// 对拍失配（查询面与账本不一致）
    CrosscheckBreach,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl IdxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IdxCode::KeyRange => "键格式越界：判据号须 4 位（1000~9999）、断言号须 3 位（0~999）",
            IdxCode::DuplicateKey => "复合键重复：全域唯一性红线，建议登记前按复合键查重",
            IdxCode::StatusRegress => "状态倒退：绿→红→封单向单调，建议新开一代索引条目",
            IdxCode::UnknownKey => "键不在账：建议先登记再置状态或查询",
            IdxCode::CrosscheckBreach => "对拍失配：查询面与账本不一致，建议重建索引再对拍",
            IdxCode::InjectLeaked => "注入漏抓：违约样本未检出，建议对齐冻结语料逐枚复核",
            IdxCode::NotReproducible => "双跑不一致：复现红线，建议固定种子后重跑两遍对账",
        }
    }
    pub const ALL: [IdxCode; 7] = [
        IdxCode::KeyRange,
        IdxCode::DuplicateKey,
        IdxCode::StatusRegress,
        IdxCode::UnknownKey,
        IdxCode::CrosscheckBreach,
        IdxCode::InjectLeaked,
        IdxCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回；段外码显性报未知）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IdxCode::ALL.len() {
        if let Some(c) = IdxCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75D40 段闭集内"
}

// ── 复合键（判据号 4 位 + 断言号 3 位） ─────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AsrtKey {
    /// 判据号（4 位：1000~9999）。
    pub criterion: u16,
    /// 断言号（3 位：0~999）。
    pub assert_no: u16,
}

impl AsrtKey {
    pub fn composite(&self) -> u32 {
        self.criterion as u32 * KEY_SCALE + self.assert_no as u32
    }
}

/// 键格式闸（登记与查询共用）。
pub fn key_valid(criterion: u16, assert_no: u16) -> bool {
    criterion >= CRIT_MIN && criterion <= CRIT_MAX && assert_no <= ASSERT_MAX
}

// ── 状态闭集（单调：绿→红→封，倒退显性拒） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsrtStatus {
    /// 绿（常绿基线）
    Green,
    /// 红（检出违约）
    Red,
    /// 封（验收封账）
    Sealed,
}

impl AsrtStatus {
    pub fn val(self) -> u32 {
        self as u32
    }
    pub const ALL: [AsrtStatus; 3] = [AsrtStatus::Green, AsrtStatus::Red, AsrtStatus::Sealed];

    /// 单调序位（绿 0 / 红 1 / 封 2）。
    pub fn rank(self) -> u32 {
        let mut i = 0;
        while i < AsrtStatus::ALL.len() {
            if let Some(s) = AsrtStatus::ALL.get(i) {
                if *s == self {
                    return i as u32;
                }
            }
            i += 1;
        }
        0
    }

    /// 前进合法谓词（同级亦拒：无操作式变更视为违约）。
    pub fn can_advance(self, to: AsrtStatus) -> bool {
        to.rank() > self.rank()
    }
}

// ── 账本（索引条目 + 拒账） ─────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub key: AsrtKey,
    pub seg: u32,
    pub status: AsrtStatus,
}

#[derive(Clone, Debug, Default)]
pub struct AssertIndex {
    pub entries: Vec<IndexEntry>,
    pub dup_rejects: u32,
    pub regress_rejects: u32,
}

impl AssertIndex {
    pub fn new() -> AssertIndex {
        AssertIndex::default()
    }

    /// 登记（键格式闸 + 复合键全域查重；初始态绿）。
    pub fn register(&mut self, key: AsrtKey, seg: u32) -> Result<(), u32> {
        if !key_valid(key.criterion, key.assert_no) {
            return Err(IdxCode::KeyRange.val());
        }
        let comp = key.composite();
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key.composite() == comp {
                    self.dup_rejects = self.dup_rejects.saturating_add(1);
                    return Err(IdxCode::DuplicateKey.val());
                }
            }
            i += 1;
        }
        self.entries.push(IndexEntry { key, seg, status: AsrtStatus::Green });
        Ok(())
    }

    /// 置状态（只许前进；倒退/同级显性拒）。
    pub fn set_status(&mut self, key: AsrtKey, to: AsrtStatus) -> Result<(), u32> {
        if !key_valid(key.criterion, key.assert_no) {
            return Err(IdxCode::KeyRange.val());
        }
        let comp = key.composite();
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                if e.key.composite() == comp {
                    if e.status.can_advance(to) {
                        e.status = to;
                        return Ok(());
                    }
                    self.regress_rejects = self.regress_rejects.saturating_add(1);
                    return Err(IdxCode::StatusRegress.val());
                }
            }
            i += 1;
        }
        Err(IdxCode::UnknownKey.val())
    }

    /// 唯一性常绿谓词：全域 distinct 复合键数 == 账面条数。
    pub fn uniqueness_ok(&self) -> bool {
        let n = self.entries.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                let a = match self.entries.get(i) {
                    Some(e) => e.key.composite(),
                    None => 0,
                };
                let b = match self.entries.get(j) {
                    Some(e) => e.key.composite(),
                    None => 1,
                };
                if a == b {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

// ── 查询面（三维度；键格式非法显性拒，合法无命中显性回空） ──────
/// 按判据号查询（返回复合键列表，账面序）。
pub fn query_by_criterion(idx: &AssertIndex, criterion: u16) -> Result<Vec<u32>, u32> {
    if criterion < CRIT_MIN || criterion > CRIT_MAX {
        return Err(IdxCode::KeyRange.val());
    }
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < idx.entries.len() {
        if let Some(e) = idx.entries.get(i) {
            if e.key.criterion == criterion {
                out.push(e.key.composite());
            }
        }
        i += 1;
    }
    Ok(out)
}

/// 按段位查询（返回复合键列表，账面序）。
pub fn query_by_segment(idx: &AssertIndex, seg: u32) -> Result<Vec<u32>, u32> {
    if seg < 0x75000 || seg > 0x781C0 || seg % 0x40 != 0 {
        return Err(IdxCode::KeyRange.val());
    }
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < idx.entries.len() {
        if let Some(e) = idx.entries.get(i) {
            if e.seg == seg {
                out.push(e.key.composite());
            }
        }
        i += 1;
    }
    Ok(out)
}

/// 按状态查询（返回复合键列表，账面序）。
pub fn query_by_status(idx: &AssertIndex, status: AsrtStatus) -> Result<Vec<u32>, u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < idx.entries.len() {
        if let Some(e) = idx.entries.get(i) {
            if e.status == status {
                out.push(e.key.composite());
            }
        }
        i += 1;
    }
    Ok(out)
}

/// 独立重扫（对拍另一面：倒序扫账本重排）。
fn rescan_by_criterion(idx: &AssertIndex, criterion: u16) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = idx.entries.len();
    while i > 0 {
        i -= 1;
        if let Some(e) = idx.entries.get(i) {
            if e.key.criterion == criterion {
                out.push(e.key.composite());
            }
        }
    }
    // 倒序收集后反转回账面序。
    let mut fwd: Vec<u32> = Vec::new();
    let mut j = out.len();
    while j > 0 {
        j -= 1;
        if let Some(x) = out.get(j) {
            fwd.push(*x);
        }
    }
    fwd
}

fn rescan_by_segment(idx: &AssertIndex, seg: u32) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = idx.entries.len();
    while i > 0 {
        i -= 1;
        if let Some(e) = idx.entries.get(i) {
            if e.seg == seg {
                out.push(e.key.composite());
            }
        }
    }
    let mut fwd: Vec<u32> = Vec::new();
    let mut j = out.len();
    while j > 0 {
        j -= 1;
        if let Some(x) = out.get(j) {
            fwd.push(*x);
        }
    }
    fwd
}

fn rescan_by_status(idx: &AssertIndex, status: AsrtStatus) -> Vec<u32> {
    let mut out: Vec<u32> = Vec::new();
    let mut i = idx.entries.len();
    while i > 0 {
        i -= 1;
        if let Some(e) = idx.entries.get(i) {
            if e.status == status {
                out.push(e.key.composite());
            }
        }
    }
    let mut fwd: Vec<u32> = Vec::new();
    let mut j = out.len();
    while j > 0 {
        j -= 1;
        if let Some(x) = out.get(j) {
            fwd.push(*x);
        }
    }
    fwd
}

fn vec_eq(a: &Vec<u32>, b: &Vec<u32>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 三维度对拍（一致率 100% 语义）：查询面 vs 独立重扫。
pub fn crosscheck_dims(idx: &AssertIndex, criterions: &[u16], segs: &[u32]) -> Result<u32, u32> {
    let mut matched = 0u32;
    let mut total = 0u32;
    // 维度 1：按判据号。
    let mut i = 0;
    while i < criterions.len() {
        if let Some(c) = criterions.get(i) {
            let q = query_by_criterion(idx, *c)?;
            let r = rescan_by_criterion(idx, *c);
            total = total.saturating_add(1);
            if vec_eq(&q, &r) {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    // 维度 2：按段位。
    let mut i = 0;
    while i < segs.len() {
        if let Some(s) = segs.get(i) {
            if *s >= 0x75000 && *s <= 0x781C0 && *s % 0x40 == 0 {
                let q = query_by_segment(idx, *s)?;
                let r = rescan_by_segment(idx, *s);
                total = total.saturating_add(1);
                if vec_eq(&q, &r) {
                    matched = matched.saturating_add(1);
                }
            }
        }
        i += 1;
    }
    // 维度 3：按状态。
    let mut i = 0;
    while i < AsrtStatus::ALL.len() {
        if let Some(s) = AsrtStatus::ALL.get(i) {
            let q = query_by_status(idx, *s)?;
            let r = rescan_by_status(idx, *s);
            total = total.saturating_add(1);
            if vec_eq(&q, &r) {
                matched = matched.saturating_add(1);
            }
        }
        i += 1;
    }
    if matched != total {
        return Err(IdxCode::CrosscheckBreach.val());
    }
    Ok(matched)
}

// ── 语料（冻结 12 条：4 判据 × 3 断言） ─────────────────────────
/// 语料判据号（4 位）。
pub const CORPUS_CRITS: [u16; 4] = [4888, 4889, 4890, 1490];
/// 语料断言号（3 位）。
pub const CORPUS_ASSERTS: [u16; 3] = [1, 2, 3];
/// 语料段位（两段位交错：0x75000 / 0x75D40，均在守卫窗内）。
pub const CORPUS_SEGS: [u32; 2] = [0x75000, 0x75D40];
/// 语料晋升表（复合键 → 目标态；未列出的保持绿）。
pub const PROMOTE_RED: [(u16, u16); 3] = [(4888, 1), (4889, 2), (4890, 3)];
pub const PROMOTE_SEALED: [(u16, u16); 2] = [(1490, 1), (4888, 2)];

/// 语料索引（登记 12 条 + 晋升红/封）。
pub fn corpus_index() -> Result<AssertIndex, u32> {
    let mut idx = AssertIndex::new();
    let mut ci = 0;
    while ci < CORPUS_CRITS.len() {
        let crit = match CORPUS_CRITS.get(ci) {
            Some(x) => *x,
            None => break,
        };
        let mut ai = 0;
        while ai < CORPUS_ASSERTS.len() {
            let ano = match CORPUS_ASSERTS.get(ai) {
                Some(x) => *x,
                None => break,
            };
            // 段位交错：复合键偶数位取段 0，奇数位取段 1。
            let seg = match CORPUS_SEGS.get((ci + ai) % 2) {
                Some(s) => *s,
                None => SEG_BASE,
            };
            idx.register(AsrtKey { criterion: crit, assert_no: ano }, seg)?;
            ai += 1;
        }
        ci += 1;
    }
    let mut i = 0;
    while i < PROMOTE_RED.len() {
        if let Some((c, a)) = PROMOTE_RED.get(i) {
            idx.set_status(AsrtKey { criterion: *c, assert_no: *a }, AsrtStatus::Red)?;
        }
        i += 1;
    }
    let mut i = 0;
    while i < PROMOTE_SEALED.len() {
        if let Some((c, a)) = PROMOTE_SEALED.get(i) {
            idx.set_status(AsrtKey { criterion: *c, assert_no: *a }, AsrtStatus::Sealed)?;
        }
        i += 1;
    }
    Ok(idx)
}

// ── LCG（种子写死 = SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：重复键 / 状态倒退 / 键格式越界） ──────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t = t.saturating_add(1);
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：语料登记→唯一性→晋升→三维度对拍→越界显性→注入必抓 ─
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let idx = corpus_index()?;
    if idx.entries.len() != CORPUS_N {
        return Err(IdxCode::InjectLeaked.val());
    }
    if !idx.uniqueness_ok() {
        return Err(IdxCode::DuplicateKey.val());
    }
    // 三维度对拍 100%。
    let matched = crosscheck_dims(&idx, &CORPUS_CRITS, &CORPUS_SEGS)?;
    if matched != (CORPUS_CRITS.len() + CORPUS_SEGS.len() + AsrtStatus::ALL.len()) as u32 {
        return Err(IdxCode::CrosscheckBreach.val());
    }
    // 重复键注入：复合键 (4888,1) 再登记显性拒。
    match idx_dup_probe() {
        Err(e) if e == IdxCode::DuplicateKey.val() => {}
        _ => return Err(IdxCode::InjectLeaked.val()),
    }
    // 状态倒退注入：封账态回退绿显性拒。
    let mut idx2 = corpus_index()?;
    match idx2.set_status(AsrtKey { criterion: 1490, assert_no: 1 }, AsrtStatus::Green) {
        Err(e) if e == IdxCode::StatusRegress.val() => {}
        _ => return Err(IdxCode::InjectLeaked.val()),
    }
    // 越界查询显性：非法键拒 / 合法无命中显性回空。
    match query_by_criterion(&idx, 999) {
        Err(e) if e == IdxCode::KeyRange.val() => {}
        _ => return Err(IdxCode::InjectLeaked.val()),
    }
    match query_by_criterion(&idx, 5000) {
        Ok(v) if v.is_empty() => {}
        _ => return Err(IdxCode::InjectLeaked.val()),
    }
    let dup_rejects = idx_dup_rejects();
    Ok((idx.entries.len() as u32, matched, dup_rejects))
}

/// 重复键探针（独立登记簿，拒账独立可查）。
fn idx_dup_probe() -> Result<(), u32> {
    let mut idx = AssertIndex::new();
    idx.register(AsrtKey { criterion: 4888, assert_no: 1 }, SEG_BASE)?;
    idx.register(AsrtKey { criterion: 4888, assert_no: 1 }, SEG_BASE)
}

/// 重复键拒账读数。
fn idx_dup_rejects() -> u32 {
    let mut idx = AssertIndex::new();
    let _ = idx.register(AsrtKey { criterion: 4888, assert_no: 1 }, SEG_BASE);
    let _ = idx.register(AsrtKey { criterion: 4888, assert_no: 1 }, SEG_BASE);
    idx.dup_rejects
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链（12 条 + 对拍 9 维全一致 + 拒账 1）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((12, 9, 1)) => Ok(()),
        _ => Err(IdxCode::NotReproducible.val()),
    }
}

/// 序 B：合法键无命中显性回空（判据号 5000 / 段位 0x75040）。
pub fn probe_seq_b() -> Result<(), u32> {
    let idx = corpus_index()?;
    let q1 = query_by_criterion(&idx, 5000)?;
    let q2 = query_by_segment(&idx, 0x75040)?;
    if q1.is_empty() && q2.is_empty() {
        Ok(())
    } else {
        Err(IdxCode::NotReproducible.val())
    }
}

/// 序 C：键格式越界显性拒（判据号 999 / 10000，断言号 1000）。
pub fn probe_seq_c() -> Result<(), u32> {
    let idx = corpus_index()?;
    let r1 = query_by_criterion(&idx, 999);
    let r2 = query_by_criterion(&idx, 10000);
    let mut reg = AssertIndex::new();
    let r3 = reg.register(AsrtKey { criterion: 1000, assert_no: 1000 }, SEG_BASE);
    match (r1, r2, r3) {
        (Err(e1), Err(e2), Err(e3))
            if e1 == IdxCode::KeyRange.val()
                && e2 == IdxCode::KeyRange.val()
                && e3 == IdxCode::KeyRange.val() =>
        {
            Ok(())
        }
        _ => Err(IdxCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 3 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重复复合键。
        match idx_dup_probe() {
            Err(e) => led.record(0, e == IdxCode::DuplicateKey.val()),
            Ok(()) => led.record(0, false),
        }
        // 路 2：状态倒退（封 → 绿）与同级变更（绿 → 绿）。
        let mut idx = corpus_index()?;
        let hit = match idx.set_status(AsrtKey { criterion: 1490, assert_no: 1 }, AsrtStatus::Green) {
            Err(e) => e == IdxCode::StatusRegress.val(),
            Ok(()) => false,
        } && match idx.set_status(AsrtKey { criterion: 4889, assert_no: 1 }, AsrtStatus::Green) {
            Err(e) => e == IdxCode::StatusRegress.val(),
            Ok(()) => false,
        };
        led.record(1, hit);
        // 路 3：键格式越界（判据号 999 + 断言号 1000 双口径）。
        let mut reg = AssertIndex::new();
        let hit3 = match reg.register(AsrtKey { criterion: 999, assert_no: 1 }, SEG_BASE) {
            Err(e) => e == IdxCode::KeyRange.val(),
            Ok(()) => false,
        } && match reg.register(AsrtKey { criterion: 1000, assert_no: 1000 }, SEG_BASE) {
            Err(e) => e == IdxCode::KeyRange.val(),
            Ok(()) => false,
        };
        led.record(2, hit3);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IdxCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在 0x75000~0x781C0 的 0x40 对齐窗口） ─────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < IdxCode::ALL.len() {
        if let Some(c) = IdxCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 诊断码闭集对账 ─────────────────────────────────────────────
fn code_closure_ok() -> bool {
    let mut i = 0;
    while i < IdxCode::ALL.len() {
        if let Some(c) = IdxCode::ALL.get(i) {
            if detail(c.val()) != c.detail() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 键格式闸对账（边界值 1000/9999/0/999 合法，999/10000/1000 断言非法） ──
fn key_validity_ok() -> bool {
    key_valid(1000, 0) && key_valid(9999, 999) && !key_valid(999, 0) && !key_valid(10000, 0)
        && !key_valid(1000, 1000) && !key_valid(0, 0)
}

// ── LCG 确定性 ─────────────────────────────────────────────────
fn lcg_deterministic_ok() -> bool {
    let mut x = Lcg::new(SEG_BASE as u64);
    let mut y = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < 16 {
        if x.next() != y.next() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 口径冻结。
    v.push((
        "J1-口径冻结",
        CRIT_MIN == 1000 && CRIT_MAX == 9999 && ASSERT_MAX == 999 && KEY_SCALE == 1000 && CORPUS_N == 12,
        format!("判据号4位+断言号3位 语料{}条", CORPUS_N),
    ));
    // 2. 键格式闸对账。
    v.push((
        "J1-键格式闸对账",
        key_validity_ok(),
        String::from("1000/9999/0/999 合法 越界值显性拒"),
    ));
    // 3. 语料账。
    let idx = corpus_index();
    v.push((
        "J1-语料账",
        match &idx {
            Ok(i) => i.entries.len() == CORPUS_N && i.dup_rejects == 0 && i.regress_rejects == 0,
            Err(_) => false,
        },
        match &idx {
            Ok(i) => format!("entries={} 拒账=0/0", i.entries.len()),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 唯一性常绿。
    v.push((
        "J1-唯一性常绿",
        match &idx {
            Ok(i) => i.uniqueness_ok(),
            Err(_) => false,
        },
        format!("{} 条复合键全域 distinct", CORPUS_N),
    ));
    // 5. 状态闭集单调（语料晋升后红 3 封 2 绿 7）。
    let status_tally_ok = match &idx {
        Ok(i) => {
            let red = query_by_status(i, AsrtStatus::Red).unwrap_or_default().len();
            let sealed = query_by_status(i, AsrtStatus::Sealed).unwrap_or_default().len();
            let green = query_by_status(i, AsrtStatus::Green).unwrap_or_default().len();
            red == 3 && sealed == 2 && green == 7
        }
        Err(_) => false,
    };
    v.push((
        "J1-状态三态账",
        status_tally_ok,
        String::from("绿7 红3 封2（晋升表落账）"),
    ));
    // 6. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((entries, matched, dup)) => *entries == 12 && *matched == 9 && *dup == 1,
            Err(_) => false,
        },
        match &g {
            Ok((entries, matched, dup)) => format!("entries={} matched={} dup拒={}", entries, matched, dup),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 7. 三维度对拍 100%。
    let cross = match &idx {
        Ok(i) => crosscheck_dims(i, &CORPUS_CRITS, &CORPUS_SEGS),
        Err(_) => Err(IdxCode::CrosscheckBreach.val()),
    };
    v.push((
        "J1-三维度对拍100%",
        match &cross {
            Ok(m) => *m == 9,
            Err(_) => false,
        },
        match &cross {
            Ok(m) => format!("{}/9 维一致（判据4+段位2+状态3）", m),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 8. 越界查询显性。
    let boundary = probe_seq_b().is_ok() && probe_seq_c().is_ok();
    v.push((
        "J1-越界查询显性",
        boundary,
        String::from("非法键显性拒 合法无命中显性回空"),
    ));
    // 9. 注入三路 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入三路10/10",
        match &inj {
            Ok(l) => l.all_caught() && slot0(&l.tries) == JUDGE_ROUNDS,
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    v.push((
        "J1-探针三序",
        a && boundary,
        format!("a={} b回空={} c键拒={}", a, probe_seq_b().is_ok(), probe_seq_c().is_ok()),
    ));
    // 11. 诊断码闭集对账。
    v.push((
        "J1-诊断码闭集对账",
        code_closure_ok(),
        format!("{} 码全查表通过", IdxCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. LCG 确定性。
    v.push((
        "J2-LCG确定性",
        lcg_deterministic_ok(),
        String::from("同种子 16 步逐位一致"),
    ));
    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75D40 窗口 0x40 对齐区间三闸"),
    ));
    v
}

/// 注入账 0 号槽取值（.get 取，不裸下标）。
fn slot0(t: &[u32; 3]) -> u32 {
    match t.get(0) {
        Some(x) => *x,
        None => 0,
    }
}

pub fn run_unx1490_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1490 域级断言索引 v1（全域唯一键与查询面）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_green() {
        match golden_run() {
            Ok((entries, matched, dup)) => {
                assert_eq!(entries, 12);
                assert_eq!(matched, 9);
                assert_eq!(dup, 1);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn frozen_consts() {
        assert_eq!(CORPUS_CRITS.len(), 4);
        assert_eq!(CORPUS_ASSERTS.len(), 3);
        assert_eq!(CORPUS_N, 12);
        assert_eq!(AsrtStatus::ALL.len(), 3);
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn composite_key_arithmetic() {
        let k = AsrtKey { criterion: 4888, assert_no: 42 };
        assert_eq!(k.composite(), 4888042);
        let k2 = AsrtKey { criterion: 1490, assert_no: 999 };
        assert_eq!(k2.composite(), 1490999);
        assert!(key_validity_ok());
    }

    #[test]
    fn uniqueness_evergreen() {
        let idx = corpus_index().ok().unwrap_or_else(|| AssertIndex::new());
        assert!(idx.uniqueness_ok());
        assert_eq!(idx.entries.len(), CORPUS_N);
        // 再登一枚重复键：唯一性必须仍常绿，拒账 +1。
        let mut idx2 = idx.clone();
        let before = idx2.entries.len();
        match idx2.register(AsrtKey { criterion: 4888, assert_no: 1 }, SEG_BASE) {
            Err(e) => assert_eq!(e, IdxCode::DuplicateKey.val()),
            Ok(()) => assert!(false, "重复键未拒"),
        }
        assert_eq!(idx2.entries.len(), before);
        assert_eq!(idx2.dup_rejects, 1);
        assert!(idx2.uniqueness_ok());
    }

    #[test]
    fn status_monotonic_gate() {
        assert_eq!(AsrtStatus::Green.rank(), 0);
        assert_eq!(AsrtStatus::Red.rank(), 1);
        assert_eq!(AsrtStatus::Sealed.rank(), 2);
        assert!(AsrtStatus::Green.can_advance(AsrtStatus::Red));
        assert!(AsrtStatus::Red.can_advance(AsrtStatus::Sealed));
        assert!(!AsrtStatus::Sealed.can_advance(AsrtStatus::Green));
        assert!(!AsrtStatus::Green.can_advance(AsrtStatus::Green));
        let mut idx = corpus_index().ok().unwrap_or_else(|| AssertIndex::new());
        match idx.set_status(AsrtKey { criterion: 1490, assert_no: 1 }, AsrtStatus::Green) {
            Err(e) => assert_eq!(e, IdxCode::StatusRegress.val()),
            Ok(()) => assert!(false, "封账倒退未拒"),
        }
        match idx.set_status(AsrtKey { criterion: 9999, assert_no: 1 }, AsrtStatus::Red) {
            Err(e) => assert_eq!(e, IdxCode::UnknownKey.val()),
            Ok(()) => assert!(false, "不在账键未拒"),
        }
    }

    #[test]
    fn crosscheck_three_dims() {
        let idx = corpus_index().ok().unwrap_or_else(|| AssertIndex::new());
        match crosscheck_dims(&idx, &CORPUS_CRITS, &CORPUS_SEGS) {
            Ok(m) => assert_eq!(m, 9),
            Err(e) => assert!(false, "对拍 err=0x{:x}", e),
        }
        // 按判据号 4888 命中 3 条（账面序）。
        match query_by_criterion(&idx, 4888) {
            Ok(v) => {
                assert_eq!(v.len(), 3);
                assert_eq!(v.first().copied(), Some(4888001));
            }
            Err(e) => assert!(false, "查询 err=0x{:x}", e),
        }
    }

    #[test]
    fn boundary_queries() {
        let idx = corpus_index().ok().unwrap_or_else(|| AssertIndex::new());
        // 合法无命中：显性回空。
        assert!(query_by_criterion(&idx, 5000).unwrap_or_default().is_empty());
        assert!(query_by_segment(&idx, 0x75040).unwrap_or_default().is_empty());
        // 非法键：显性拒。
        assert!(query_by_criterion(&idx, 999).is_err());
        assert!(query_by_criterion(&idx, 10000).is_err());
        assert!(query_by_segment(&idx, 0x75D41).is_err());
        assert!(query_by_segment(&idx, 0x74000).is_err());
    }

    #[test]
    fn inject_rounds_tally() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(e) => {
                assert!(false, "inject err=0x{:x}", e);
                return;
            }
        };
        assert!(led.all_caught());
        assert_eq!(slot0(&led.tries), JUDGE_ROUNDS);
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(code_closure_ok());
        assert!(lcg_deterministic_ok());
        assert_eq!(detail(IdxCode::DuplicateKey.val()), IdxCode::DuplicateKey.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
