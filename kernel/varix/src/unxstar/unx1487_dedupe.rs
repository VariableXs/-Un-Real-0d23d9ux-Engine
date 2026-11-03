//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1487 · 断言去重与合并语义 · 0x75B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批4-J）：
//!   跨批同判据号重复断言归并（主注册+引用注册两类），去重后总账零
//!   重复键，误合并注入 10/10 必抓
//! 设计要点：
//!   1. 判据号闭集：冻结 8 键（101~108），四批原始 feed（批 0 主注册
//!      8 行 + 批 1~3 重复引用 10 行 = 18 行）确定性给定；
//!   2. 两类注册语义：同判据号首见即主注册（Primary），跨批重复见
//!      归并为引用注册（Reference）——主注册键全账唯一；
//!   3. 误合并红线：merge_pair 判据号不同即显性拒（MisMerge），
//!      注入 10/10 必抓；重复主注册 / 无主引用两类违约同样显性拒；
//!   4. 去重台账随册归档：总账行 = 主注册行 + 引用注册行（守恒式
//!      逐次归并后复算），零重复键断言入总账；
//!   5. 归并断言 append-only 入总账（seq 单调），只增不减双闸审计；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75B80，AI-VX7-1104 批4-J 独占槽）。
pub const SEG_BASE: u32 = 0x75B80;

// ── 判据号闭集与 feed 冻结口径 ──────────────────────────────────
/// 判据号闭集（冻结 8 键）。
pub const CRIT_KEYS: [u32; 8] = [101, 102, 103, 104, 105, 106, 107, 108];
/// 归并源批闭集（四批原始 feed）。
pub const FEED_BATCHES: [&str; 4] = ["B41", "B42", "B43", "B44"];
/// 主注册行冻结数（每键恰一主）。
pub const PRIMARY_ROWS: usize = 8;
/// 引用注册行冻结数（跨批重复归并口径）。
pub const REFERENCE_ROWS: usize = 10;
/// 总账行守恒口径（主 + 引 = 18）。
pub const TOTAL_ROWS: usize = 18;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DdCode {
    /// 判据号非法（0 号键）
    KeyZero,
    /// 重复主注册（同键第二主）
    DuplicateKey,
    /// 无主引用（引用注册前无主注册）
    KeyNotFound,
    /// 误合并（不同判据号强归并）
    MisMerge,
    /// 台账守恒破坏（行数 ≠ 主 + 引）
    ConservationBreach,
    /// 空断言语料
    EmptyText,
    /// 总账删除（append-only / seq 单调红线）
    LedgerDeletion,
    /// 注入漏抓
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl DdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DdCode::KeyZero => "判据号非法：0 号键不入册，建议以 CRIT_KEYS 闭集对号",
            DdCode::DuplicateKey => "重复主注册：同判据号仅一主，建议后来行降级为引用注册归并",
            DdCode::KeyNotFound => "无主引用：引用注册前须有主注册，建议先补主行再引",
            DdCode::MisMerge => "误合并：不同判据号不得归并，建议 merge 前先对键相等性把关",
            DdCode::ConservationBreach => "台账守恒破坏：总账行 ≠ 主注册行 + 引用注册行，建议归并后即席复算",
            DdCode::EmptyText => "空断言语料：语料不得为空，建议每行带判据号+批名可对账键",
            DdCode::LedgerDeletion => "总账删除：append-only / seq 单调红线，建议只追加不回撤",
            DdCode::InjectLeaked => "注入漏抓：违约样本未被拦截，建议复核守卫谓词方向",
            DdCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定性来源（随机/墙钟/迭代序）",
        }
    }
    pub const ALL: [DdCode; 9] = [
        DdCode::KeyZero,
        DdCode::DuplicateKey,
        DdCode::KeyNotFound,
        DdCode::MisMerge,
        DdCode::ConservationBreach,
        DdCode::EmptyText,
        DdCode::LedgerDeletion,
        DdCode::InjectLeaked,
        DdCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if let Some(c) = DdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75B80 段闭集内，建议核对诊断码表"
}

// ── 注册类别与断言行 ────────────────────────────────────────────
/// 注册两类：主注册（键首见）/ 引用注册（跨批重复归并）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegClass {
    /// 主注册（键全账唯一）
    Primary,
    /// 引用注册（同键跨批重复的归并位）
    Reference,
}

impl RegClass {
    /// 中文类名（账面文案）。
    pub fn name(self) -> &'static str {
        match self {
            RegClass::Primary => "主注册",
            RegClass::Reference => "引用注册",
        }
    }
}

/// 归并后总账行：seq 单调 + 判据号 + 批 + 类 + 语料。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssertEntry {
    pub seq: u32,
    pub key: u32,
    pub batch: usize,
    pub class: RegClass,
    pub text: String,
}

/// 去重总账（append-only，零重复主键）。
#[derive(Clone, Debug)]
pub struct DedupeLedger {
    pub entries: Vec<AssertEntry>,
    next_seq: u32,
}

impl DedupeLedger {
    pub fn new() -> DedupeLedger {
        DedupeLedger { entries: Vec::new(), next_seq: 0 }
    }

    /// 该键是否已有主注册。
    pub fn has_primary(&self, key: u32) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == key && e.class == RegClass::Primary {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 登记一行（键零/空语料/重复主/无主引用四闸 + seq 单调追加）。
    pub fn register(&mut self, key: u32, batch: usize, class: RegClass, text: &str) -> Result<u32, u32> {
        if key == 0 {
            return Err(DdCode::KeyZero.val());
        }
        if text.is_empty() {
            return Err(DdCode::EmptyText.val());
        }
        match class {
            RegClass::Primary => {
                if self.has_primary(key) {
                    return Err(DdCode::DuplicateKey.val());
                }
            }
            RegClass::Reference => {
                if !self.has_primary(key) {
                    return Err(DdCode::KeyNotFound.val());
                }
            }
        }
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(DdCode::LedgerDeletion.val()),
        };
        let bname = match FEED_BATCHES.get(batch) {
            Some(n) => *n,
            None => "",
        };
        self.entries.push(AssertEntry {
            seq,
            key,
            batch,
            class,
            text: format!("断言 K{} @{} · {}", key, bname, text),
        });
        Ok(seq)
    }

    /// 归并语义核心：同键两行归并（首行主、后行引），异键即误合并显性拒。
    pub fn merge_pair(&mut self, a_key: u32, b_key: u32, batch: usize, text: &str) -> Result<u32, u32> {
        if a_key != b_key {
            return Err(DdCode::MisMerge.val());
        }
        // 同键：首见主注册，重复见引用注册（归并语义）。
        if self.has_primary(a_key) {
            self.register(a_key, batch, RegClass::Reference, text)
        } else {
            self.register(a_key, batch, RegClass::Primary, text)
        }
    }

    /// 跨批原始 feed 归并执行：逐行走 merge_pair 同键语义。
    pub fn merge_feed(&mut self, feed: &[(usize, u32, &str)]) -> Result<usize, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < feed.len() {
            let (batch, key, text) = match feed.get(i) {
                Some((b, k, t)) => (*b, *k, *t),
                None => return Err(DdCode::LedgerDeletion.val()),
            };
            self.merge_pair(key, key, batch, text)?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 行分类计数：(主注册行, 引用注册行)。
    pub fn class_counts(&self) -> (usize, usize) {
        let mut p = 0;
        let mut r = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                match e.class {
                    RegClass::Primary => p += 1,
                    RegClass::Reference => r += 1,
                }
            }
            i += 1;
        }
        (p, r)
    }

    /// 台账守恒：总账行 = 主注册行 + 引用注册行（分区无漏）。
    pub fn conservation_ok(&self) -> bool {
        let (p, r) = self.class_counts();
        match p.checked_add(r) {
            Some(sum) => sum == self.entries.len(),
            None => false,
        }
    }

    /// 零重复键断言：主注册键全账唯一（去重后口径）。
    pub fn no_duplicate_keys(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            let (ki, ci) = match self.entries.get(i) {
                Some(e) => (e.key, e.class),
                None => return false,
            };
            if ci == RegClass::Primary {
                let mut j = i + 1;
                while j < self.entries.len() {
                    if let Some(e) = self.entries.get(j) {
                        if e.key == ki && e.class == RegClass::Primary {
                            return false;
                        }
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        true
    }

    /// 只增不减审计：行数不减 + 前缀稳定 + seq 全列单调。
    pub fn append_only_audit(&self, prev: &[AssertEntry]) -> Result<(), u32> {
        if self.entries.len() < prev.len() {
            return Err(DdCode::LedgerDeletion.val());
        }
        let mut i = 0;
        while i < prev.len() {
            match (prev.get(i), self.entries.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return Err(DdCode::LedgerDeletion.val());
                    }
                }
                _ => return Err(DdCode::LedgerDeletion.val()),
            }
            i += 1;
        }
        let mut j = 1;
        while j < self.entries.len() {
            let p = match self.entries.get(j - 1) {
                Some(x) => x.seq,
                None => return Err(DdCode::LedgerDeletion.val()),
            };
            let c = match self.entries.get(j) {
                Some(x) => x.seq,
                None => return Err(DdCode::LedgerDeletion.val()),
            };
            if c <= p {
                return Err(DdCode::LedgerDeletion.val());
            }
            j += 1;
        }
        Ok(())
    }
}

impl Default for DedupeLedger {
    fn default() -> DedupeLedger {
        DedupeLedger::new()
    }
}

// ── 原始 feed 闭集（四批确定性给定：8 主 + 10 引 = 18 行） ──────
/// 跨批原始 feed：(批, 判据号, 语料键)。批 0 主注册 8 行，
/// 批 1/2/3 对同键重复登记（归并为引用）。
pub fn raw_feed() -> Vec<(usize, u32, &'static str)> {
    let mut f: Vec<(usize, u32, &'static str)> = Vec::new();
    // 批 0：主注册 8 行。
    let mut k = 0;
    while k < CRIT_KEYS.len() {
        if let Some(key) = CRIT_KEYS.get(k) {
            f.push((0, *key, "首批主注册行"));
        }
        k += 1;
    }
    // 批 1：引用 101/103/105/107。
    let mut k2 = 0;
    while k2 < CRIT_KEYS.len() {
        if let Some(key) = CRIT_KEYS.get(k2) {
            if key % 2 == 1 {
                f.push((1, *key, "批1重复引用行"));
            }
        }
        k2 += 1;
    }
    // 批 2：引用 102/104/106/108。
    let mut k3 = 0;
    while k3 < CRIT_KEYS.len() {
        if let Some(key) = CRIT_KEYS.get(k3) {
            if key % 2 == 0 {
                f.push((2, *key, "批2重复引用行"));
            }
        }
        k3 += 1;
    }
    // 批 3：引用 101/108（跨批再归并）。
    if let Some(k101) = CRIT_KEYS.get(0) {
        f.push((3, *k101, "批3重复引用行"));
    }
    if let Some(k108) = CRIT_KEYS.get(7) {
        f.push((3, *k108, "批3重复引用行"));
    }
    f
}

// ── 全量归并（feed → 去重总账） ─────────────────────────────────
pub fn merge_all() -> Result<DedupeLedger, u32> {
    if CRIT_KEYS.len() != 8 || FEED_BATCHES.len() != 4 {
        return Err(DdCode::KeyZero.val());
    }
    let mut led = DedupeLedger::new();
    let feed = raw_feed();
    led.merge_feed(&feed)?;
    // 去重口径：8 主 + 10 引 = 18 行，零重复键，守恒。
    let (p, r) = led.class_counts();
    if p != PRIMARY_ROWS || r != REFERENCE_ROWS {
        return Err(DdCode::ConservationBreach.val());
    }
    if led.entries.len() != TOTAL_ROWS {
        return Err(DdCode::ConservationBreach.val());
    }
    if !led.no_duplicate_keys() || !led.conservation_ok() {
        return Err(DdCode::ConservationBreach.val());
    }
    Ok(led)
}

// ── 误合并注入 10/10（每轮异键强归并，必抓） ────────────────────
pub fn mismerge_inject_10() -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let mut led = DedupeLedger::new();
        // 建立键 101 主注册。
        let _ = led.register(101, 0, RegClass::Primary, "主行");
        // 异键强归并（102≠101）必须被拒。
        let hit = led.merge_pair(101, 102, 1, "异键行").is_err();
        // 同键归并必须放行（引用注册语义）。
        let ok_path = led.merge_pair(101, 101, 1, "同键行").is_ok();
        if hit && ok_path {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(DdCode::InjectLeaked.val());
    }
    Ok(caught)
}

// ── 重复主注册/无主引用注入 10/10 ───────────────────────────────
pub fn class_inject_10() -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let mut led = DedupeLedger::new();
        let _ = led.register(101, 0, RegClass::Primary, "主行");
        let dup_primary = led.register(101, 1, RegClass::Primary, "重主行").is_err();
        let orphan_ref = led.register(999, 1, RegClass::Reference, "无主行").is_err();
        let zero_key = led.register(0, 1, RegClass::Primary, "零键行").is_err();
        if dup_primary && orphan_ref && zero_key {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(DdCode::InjectLeaked.val());
    }
    Ok(caught)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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

// ── 注入检出账（四槽：误合并/重主/无主引用/守恒） ───────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：feed 归并→去重对账→守恒→误合并/类注入 10/10 ──────
pub fn golden_run() -> Result<(usize, usize, usize, u32), u32> {
    let led = merge_all()?;
    let (p, r) = led.class_counts();
    // 零重复键 + 守恒 + 只增审计。
    if !led.no_duplicate_keys() {
        return Err(DdCode::ConservationBreach.val());
    }
    if led.append_only_audit(&[]).is_err() {
        return Err(DdCode::LedgerDeletion.val());
    }
    let mis = mismerge_inject_10()?;
    let cls = class_inject_10()?;
    Ok((p, r, led.entries.len(), mis + cls))
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// feed 闭集谓词：18 行冻结 + 键全在闭集内。
fn feed_closed_ok() -> bool {
    let feed = raw_feed();
    if feed.len() != TOTAL_ROWS {
        return false;
    }
    let mut i = 0;
    while i < feed.len() {
        let (_, key, text) = match feed.get(i) {
            Some((_, k, t)) => (*k, *k, *t),
            None => return false,
        };
        if text.is_empty() {
            return false;
        }
        let mut found = false;
        let mut j = 0;
        while j < CRIT_KEYS.len() {
            if let Some(kc) = CRIT_KEYS.get(j) {
                if *kc == key {
                    found = true;
                }
            }
            j += 1;
        }
        if !found {
            return false;
        }
        i += 1;
    }
    true
}

/// 归并语义谓词：同键双行归并为主+引，键唯一性保持。
fn merge_semantics_ok() -> bool {
    let mut led = DedupeLedger::new();
    let s1 = led.merge_pair(103, 103, 0, "首行");
    let s2 = led.merge_pair(103, 103, 1, "次行");
    match (s1, s2) {
        (Ok(_), Ok(_)) => {
            let (p, r) = led.class_counts();
            p == 1 && r == 1 && led.no_duplicate_keys() && led.conservation_ok()
        }
        _ => false,
    }
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A（正向）：单键主注册 + 守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = DedupeLedger::new();
    led.register(101, 0, RegClass::Primary, "探针主行")?;
    if !led.conservation_ok() {
        return Err(DdCode::ConservationBreach.val());
    }
    Ok(())
}

/// 序 B（无主引用必拒）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = DedupeLedger::new();
    led.register(105, 0, RegClass::Reference, "无主行").map(|_| ())
}

/// 序 C（误合并必拒）：空账上异键强归并。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = DedupeLedger::new();
    led.merge_pair(101, 102, 0, "异键行").map(|_| ())
}

// ── 注入轮（LCG 驱动 rounds × 四槽） ────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 槽0：误合并。
        let mut l0 = DedupeLedger::new();
        let _ = l0.register(101, 0, RegClass::Primary, "主行");
        led.record(0, l0.merge_pair(101, 102, 1, "异键行").is_err());
        // 槽1：重复主注册。
        let mut l1 = DedupeLedger::new();
        let _ = l1.register(103, 0, RegClass::Primary, "主行");
        led.record(1, l1.register(103, 1, RegClass::Primary, "重主行").is_err());
        // 槽2：无主引用。
        let mut l2 = DedupeLedger::new();
        led.record(2, l2.register(107, 0, RegClass::Reference, "无主行").is_err());
        // 槽3：零键 + 空语料。
        let mut l3 = DedupeLedger::new();
        let zero = l3.register(0, 0, RegClass::Primary, "零键行").is_err();
        let blank = l3.register(101, 0, RegClass::Primary, "").is_err();
        led.record(3, zero && blank);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（窗口 + 对齐 + 区间三闸） ──────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < DdCode::ALL.len() {
        if let Some(c) = DdCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. feed 闭集。
    v.push((
        "J1-feed闭集",
        feed_closed_ok(),
        format!("四批 18 行冻结：{} 主 + {} 引", PRIMARY_ROWS, REFERENCE_ROWS),
    ));
    // 2. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((p, r, total, inj)) => *p == 8 && *r == 10 && *total == 18 && *inj == 20,
            Err(_) => false,
        },
        match &g {
            Ok((p, r, total, inj)) => format!("主={} 引={} 总={} 注抓={}", p, r, total, inj),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. 归并语义（主+引两类）。
    v.push((
        "J1-归并语义主引两类",
        merge_semantics_ok(),
        String::from("同键首见主注册 · 重复见引用注册"),
    ));
    // 4. 去重后零重复键。
    let led = merge_all();
    v.push((
        "J1-去重后零重复键",
        match &led {
            Ok(l) => l.no_duplicate_keys(),
            Err(_) => false,
        },
        match &led {
            Ok(l) => format!("主键 {} 枚全唯一", l.class_counts().0),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 台账守恒（随册归档）。
    v.push((
        "J1-台账守恒随册归档",
        match &led {
            Ok(l) => l.conservation_ok() && l.entries.len() == TOTAL_ROWS,
            Err(_) => false,
        },
        String::from("总账行 = 主注册行 + 引用注册行 = 18"),
    ));
    // 6. 误合并注入 10/10。
    v.push((
        "J1-误合并注入10/10",
        mismerge_inject_10() == Ok(10),
        format!("{}/10 轮抓", match mismerge_inject_10() {
            Ok(n) => n,
            Err(_) => 0,
        }),
    ));
    // 7. 重复主/无主引用注入 10/10。
    v.push((
        "J1-类违约注入10/10",
        class_inject_10() == Ok(10),
        String::from("重主/无主引用/零键 三路全抓"),
    ));
    // 8. 注入四槽 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入四槽10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 9. 只增不减（seq 单调）。
    let append_ok = match &led {
        Ok(l) => {
            l.append_only_audit(&[]).is_ok()
                && l.entries.first().map(|e| e.seq) == Some(0)
                && l.entries.last().map(|e| e.seq) == Some(17)
        }
        Err(_) => false,
    };
    v.push((
        "J1-总账只增不减",
        append_ok,
        String::from("append-only + seq 0..17 单调"),
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a正向={} b无主引拒={} c误合并拒={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    let codes_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DdCode::ALL.len() {
            if let Some(c) = DdCode::ALL.get(i) {
                if detail(c.val()) != c.detail() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
    };
    v.push((
        "J1-诊断码闭集对账",
        codes_ok,
        format!("{} 码全查表通过", DdCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75B80 窗口 0x40 对齐区间三闸"),
    ));
    // 14. LCG 确定性。
    let lcg_ok = {
        let mut x = Lcg::new(SEG_BASE as u64);
        let mut y = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 16 {
            if x.next() != y.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J2-LCG确定性", lcg_ok, String::from("同种子 16 步逐位一致")));
    v
}

pub fn run_unx1487_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1487 断言去重与合并语义");
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
            Ok((p, r, total, inj)) => {
                assert_eq!(p, 8);
                assert_eq!(r, 10);
                assert_eq!(total, 18);
                assert_eq!(inj, 20);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn feed_closed_shape() {
        assert!(feed_closed_ok());
        assert_eq!(raw_feed().len(), 18);
        assert_eq!(PRIMARY_ROWS + REFERENCE_ROWS, TOTAL_ROWS);
    }

    #[test]
    fn duplicate_primary_caught() {
        let mut led = DedupeLedger::new();
        assert!(led.register(102, 0, RegClass::Primary, "主行").is_ok());
        match led.register(102, 1, RegClass::Primary, "重主行") {
            Err(e) => assert_eq!(e, DdCode::DuplicateKey.val()),
            Ok(_) => assert!(false, "重复主注册未拒"),
        }
    }

    #[test]
    fn orphan_reference_caught() {
        let mut led = DedupeLedger::new();
        match led.register(104, 0, RegClass::Reference, "无主行") {
            Err(e) => assert_eq!(e, DdCode::KeyNotFound.val()),
            Ok(_) => assert!(false, "无主引用未拒"),
        }
    }

    #[test]
    fn mismerge_caught_10() {
        assert_eq!(mismerge_inject_10(), Ok(10));
        let mut led = DedupeLedger::new();
        match led.merge_pair(101, 105, 0, "异键行") {
            Err(e) => assert_eq!(e, DdCode::MisMerge.val()),
            Ok(_) => assert!(false, "误合并未拒"),
        }
    }

    #[test]
    fn key_zero_and_blank_caught() {
        let mut led = DedupeLedger::new();
        match led.register(0, 0, RegClass::Primary, "零键") {
            Err(e) => assert_eq!(e, DdCode::KeyZero.val()),
            Ok(_) => assert!(false, "零键未拒"),
        }
        match led.register(101, 0, RegClass::Primary, "") {
            Err(e) => assert_eq!(e, DdCode::EmptyText.val()),
            Ok(_) => assert!(false, "空语料未拒"),
        }
    }

    #[test]
    fn conservation_and_no_dup_keys() {
        let led = merge_all();
        match led {
            Ok(l) => {
                assert!(l.conservation_ok());
                assert!(l.no_duplicate_keys());
                let (p, r) = l.class_counts();
                assert_eq!(p, 8);
                assert_eq!(r, 10);
                assert_eq!(p + r, l.entries.len());
            }
            Err(e) => assert!(false, "merge_all err=0x{:x}", e),
        }
    }

    #[test]
    fn seq_monotonic_append_only() {
        let mut led = DedupeLedger::new();
        let s1 = led.register(101, 0, RegClass::Primary, "一");
        let s2 = led.register(101, 1, RegClass::Reference, "二");
        match (s1, s2) {
            (Ok(a), Ok(b)) => assert!(a < b, "seq 必须单调"),
            _ => assert!(false, "正常归并不应拒"),
        }
        let snap = led.entries.clone();
        assert!(led.register(102, 0, RegClass::Primary, "三").is_ok());
        assert!(led.append_only_audit(&snap).is_ok());
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn reproducible_and_guard() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(DdCode::MisMerge.val()), DdCode::MisMerge.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
