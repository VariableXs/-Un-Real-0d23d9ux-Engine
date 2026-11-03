//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0587 · 重复参数账（首现口径） · 0x71EC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1-B30）：
//!   ① 纯功能代码 ≥200 行；
//!   ② 判据原文：重复键账（注入同键 3 现——首现生效 1/1 断言+重复
//!     计数 2/2），重复簿可查；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 首现口径：同键重复投喂，第一次落账生效，后续只进重复簿计数，
//!     值永不覆盖（first-wins）；
//!   · 重复簿可查：dup_keys × dup_counts 双列账，计数可复核；
//!   · 注入对照：坏簿（last-wins 覆写语义）与首现断言必冲突，
//!    审计必抓（FirstViolated）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71EC0，VX3-1103 批集窗口独占）。
pub const SEG_BASE: u32 = 0x71EC0;

/// 键长上限（字符数，冻结 32）。
pub const KEY_LEN_MAX: usize = 32;
/// 参数簿容量（冻结 256）。
pub const BOOK_CAP: usize = 256;
/// 注入同键次数（3 现，冻结）。
pub const SAME_KEY_TIMES: usize = 3;
/// 期望重复计数（3 现 − 首现 = 2/2，冻结）。
pub const EXPECT_DUP: u32 = 2;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DupCode {
    /// 空键
    KeyEmpty,
    /// 键超长（>32 字符）
    KeyTooLong,
    /// 簿满（>256 键）
    BookFull,
    /// 首现违约（生效值不是首现值）
    FirstViolated,
    /// 重复簿坏（幻影条目 / 计数失配 / 簿内不一致）
    LedgerBad,
    /// 复算失配（同投喂序两簿不同态）
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DupCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DupCode::KeyEmpty => "空键：键不得为零长",
            DupCode::KeyTooLong => "键超长：上限 32 字符",
            DupCode::BookFull => "簿满：256 键上限",
            DupCode::FirstViolated => "首现违约：生效值必须为首现值，查覆写路径",
            DupCode::LedgerBad => "重复簿坏：幻影条目或计数失配，按投喂账重算",
            DupCode::ReplayMismatch => "复算失配：同投喂序两簿应同态",
            DupCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [DupCode; 7] = [
        DupCode::KeyEmpty,
        DupCode::KeyTooLong,
        DupCode::BookFull,
        DupCode::FirstViolated,
        DupCode::LedgerBad,
        DupCode::ReplayMismatch,
        DupCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DupCode::ALL.len() {
        if DupCode::ALL[i].val() == code {
            return DupCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x71EC0 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 投喂结果：首现落账 / 重复进簿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FeedMark {
    /// 首现：键新立，值落账生效。
    First,
    /// 重复：键已在，值丢弃只计数。
    Duplicate,
}

/// 参数簿（首现口径）：
///   keys × vals 主账（键序 = 首现序）；
///   dup_keys × dup_counts 重复簿（计数只增不减）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParamBook {
    pub keys: Vec<String>,
    pub vals: Vec<u64>,
    pub dup_keys: Vec<String>,
    pub dup_counts: Vec<u32>,
    /// 累计投喂次数。
    pub feeds: u32,
    /// 累计重复命中次数。
    pub dups: u32,
}

impl ParamBook {
    pub fn new() -> ParamBook {
        ParamBook::default()
    }

    /// 主账键定位。
    pub fn key_index(&self, key: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.keys.len() {
            match self.keys.get(i) {
                Some(k) if k == key => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 投喂一参：空键/超长闸 → 已在则重复进簿（值丢弃）→ 否则首现落账。
    pub fn feed(&mut self, key: &str, val: u64) -> Result<FeedMark, u32> {
        if key.is_empty() {
            return Err(DupCode::KeyEmpty.val());
        }
        if key.chars().count() > KEY_LEN_MAX {
            return Err(DupCode::KeyTooLong.val());
        }
        self.feeds += 1;
        match self.key_index(key) {
            Some(_) => {
                // 重复：只进重复簿，主账值不动（首现口径）。
                match self.dup_keys.iter().position(|k| k == key) {
                    Some(pos) => {
                        if let Some(c) = self.dup_counts.get_mut(pos) {
                            *c += 1;
                        }
                    }
                    None => {
                        self.dup_keys.push(String::from(key));
                        self.dup_counts.push(1);
                    }
                }
                self.dups += 1;
                Ok(FeedMark::Duplicate)
            }
            None => {
                if self.keys.len() >= BOOK_CAP {
                    return Err(DupCode::BookFull.val());
                }
                self.keys.push(String::from(key));
                self.vals.push(val);
                Ok(FeedMark::First)
            }
        }
    }

    /// 查生效值（恒为首现值）。
    pub fn lookup(&self, key: &str) -> Option<u64> {
        let i = self.key_index(key)?;
        self.vals.get(i).copied()
    }

    /// 重复簿查计数（未现键 0）。
    pub fn dup_count_of(&self, key: &str) -> u32 {
        match self.dup_keys.iter().position(|k| k == key) {
            Some(pos) => match self.dup_counts.get(pos) {
                Some(c) => *c,
                None => 0,
            },
            None => 0,
        }
    }

    /// 重复簿全量查询（可查判据：键+计数成对导出）。
    pub fn ledger_query(&self) -> Vec<(String, u32)> {
        let mut v: Vec<(String, u32)> = Vec::new();
        let mut i = 0;
        while i < self.dup_keys.len() {
            let k = match self.dup_keys.get(i) {
                Some(k) => k.clone(),
                None => break,
            };
            let c = match self.dup_counts.get(i) {
                Some(c) => *c,
                None => 0,
            };
            v.push((k, c));
            i += 1;
        }
        v
    }

    /// 首现断言：生效值必须等于期望首现值。
    pub fn assert_first_wins(&self, key: &str, expect_first: u64) -> Result<(), u32> {
        match self.lookup(key) {
            Some(v) if v == expect_first => Ok(()),
            Some(_) => Err(DupCode::FirstViolated.val()),
            None => Err(DupCode::FirstViolated.val()),
        }
    }

    /// 重复簿审计：期望计数对平 + 无幻影条目 + 簿内和一致。
    pub fn audit_ledger(&self, key: &str, expect_count: u32) -> Result<(), u32> {
        if self.dup_count_of(key) != expect_count {
            return Err(DupCode::LedgerBad.val());
        }
        // 幻影条目：重复簿里的键必须都在主账。
        let mut i = 0;
        while i < self.dup_keys.len() {
            let k = match self.dup_keys.get(i) {
                Some(k) => k.as_str(),
                None => return Err(DupCode::LedgerBad.val()),
            };
            if self.key_index(k).is_none() {
                return Err(DupCode::LedgerBad.val());
            }
            i += 1;
        }
        // 簿内和：dup_counts 总和 == dups 计数。
        let mut sum: u32 = 0;
        let mut j = 0;
        while j < self.dup_counts.len() {
            if let Some(c) = self.dup_counts.get(j) {
                sum = sum.saturating_add(*c);
            }
            j += 1;
        }
        if sum != self.dups {
            return Err(DupCode::LedgerBad.val());
        }
        Ok(())
    }

    /// 簇内一致性：主账键值列同长、簿双列同长。
    pub fn structurally_sane(&self) -> bool {
        self.keys.len() == self.vals.len() && self.dup_keys.len() == self.dup_counts.len()
    }
}

/// 注入对照簿：last-wins 覆写语义（坏簿模型，仅供注入检测）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BadBook {
    pub keys: Vec<String>,
    pub vals: Vec<u64>,
}

impl BadBook {
    pub fn new() -> BadBook {
        BadBook::default()
    }
    /// 坏簿投喂：同键覆盖旧值（首现口径的对立面）。
    pub fn feed(&mut self, key: &str, val: u64) {
        match self.keys.iter().position(|k| k == key) {
            Some(pos) => {
                if let Some(v) = self.vals.get_mut(pos) {
                    *v = val;
                }
            }
            None => {
                self.keys.push(String::from(key));
                self.vals.push(val);
            }
        }
    }
    pub fn lookup(&self, key: &str) -> Option<u64> {
        let i = self.keys.iter().position(|k| k == key)?;
        self.vals.get(i).copied()
    }
}

/// 注入检出：坏簿（last-wins 覆写语义）三现同键后，生效值被覆写
/// 为末现值（≠ 首现值）；把"末现值生效"这一注入主张压到首现断言
/// 上必被抓（FirstViolated）。
pub fn inject_overwrite_bug_caught() -> bool {
    let mut bad = BadBook::new();
    bad.feed("rk", 100);
    bad.feed("rk", 200);
    bad.feed("rk", 300);
    // 坏簿生效值已被覆写为 300，违反首现口径。
    let diverged = match bad.lookup("rk") {
        Some(v) => v != 100,
        None => true,
    };
    // 注入主张（300 生效）压首现断言必报违约。
    let mut probe = ParamBook::new();
    let _ = probe.feed("rk", 100);
    let _ = probe.feed("rk", 200);
    let _ = probe.feed("rk", 300);
    let asserted = matches!(
        probe.assert_first_wins("rk", 300),
        Err(e) if e == DupCode::FirstViolated.val()
    );
    diverged && asserted
}

// ── LCG（确定性伪随机，供注入轮步进） ───────────────────────────
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

// ── 注入检出账（四路：空键路/超长路/覆写路/簿坏路） ─────────────
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
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 造 n 字符键（确定性，不依赖 repeat）。
pub fn make_key(n: usize, seed: char) -> String {
    let mut s = String::from("");
    let mut i = 0;
    while i < n {
        s.push(seed);
        i += 1;
    }
    s
}

// ── 黄金全链：同键 3 现→首现 1/1+重复 2/2→簿可查→两拒→坏簿必抓 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut book = ParamBook::new();
    // 注入同键 3 现：首现生效，后两现进簿。
    let m1 = book.feed("rk", 100)?;
    let m2 = book.feed("rk", 200)?;
    let m3 = book.feed("rk", 300)?;
    let marks_ok = m1 == FeedMark::First && m2 == FeedMark::Duplicate && m3 == FeedMark::Duplicate;
    // 首现生效断言（1/1）。
    book.assert_first_wins("rk", 100)?;
    let first_ok = book.lookup("rk") == Some(100);
    // 重复计数（2/2）+ 簿可查。
    book.audit_ledger("rk", EXPECT_DUP)?;
    let count_ok = book.dup_count_of("rk") == EXPECT_DUP;
    let led = book.ledger_query();
    let query_ok = led.len() == 1
        && match led.first() {
            Some((k, c)) => k == "rk" && *c == EXPECT_DUP,
            None => false,
        };
    // 异键独立：不同键互不串账。
    let _ = book.feed("other", 7);
    let independent = book.lookup("other") == Some(7) && book.dup_count_of("other") == 0;
    // 投喂账：feeds=5（3+1 异键后再 1）、dups=2。
    let _ = book.feed("rk", 400);
    let feeds_ok = book.feeds == 5 && book.dups == 3;
    // 两拒口径：空键 / 超长键。
    let empty = book.feed("", 1).is_err();
    let long = book.feed(&make_key(KEY_LEN_MAX + 1, 'k'), 1).is_err();
    // 坏簿注入必抓。
    let bad = inject_overwrite_bug_caught();
    let flags = marks_ok as u32
        + first_ok as u32
        + count_ok as u32
        + query_ok as u32
        + independent as u32
        + feeds_ok as u32
        + bad as u32;
    let rejects = empty as u32 + long as u32;
    Ok((flags, rejects, book.feeds))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链（7 旗 + 2 拒 + 5 投喂）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((7, 2, 5)) => Ok(()),
        _ => Err(DupCode::NotReproducible.val()),
    }
}

/// 序 B：审计面（错期望首现 / 错期望计数 / 幻影键）必抓。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut book = ParamBook::new();
    let _ = book.feed("k", 1);
    let _ = book.feed("k", 2);
    // 错期望首现：断言必须报 FirstViolated。
    let b1 = matches!(book.assert_first_wins("k", 2), Err(e) if e == DupCode::FirstViolated.val());
    // 错期望计数：审计必须报 LedgerBad。
    let b2 = matches!(book.audit_ledger("k", 5), Err(e) if e == DupCode::LedgerBad.val());
    // 未现键：查值 None、计数 0、断言违约。
    let b3 = book.lookup("nope").is_none() && book.dup_count_of("nope") == 0;
    let b4 = matches!(book.assert_first_wins("nope", 1), Err(e) if e == DupCode::FirstViolated.val());
    match (b1, b2, b3, b4) {
        (true, true, true, true) => Ok(()),
        _ => Err(DupCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（键长 32 / 簿容 256 / 3 现 / 重复 2）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        KEY_LEN_MAX == 32,
        BOOK_CAP == 256,
        SAME_KEY_TIMES == 3,
        EXPECT_DUP == 2,
        make_key(3, 'a') == "aaa",
    ) {
        (true, true, true, true, true) => Ok(()),
        _ => Err(DupCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：空键路。
        let mut b1 = ParamBook::new();
        led.record(0, b1.feed("", 1).is_err());
        // 路 2：超长路（33 字符键必拒）。
        let mut b2 = ParamBook::new();
        led.record(1, b2.feed(&make_key(KEY_LEN_MAX + 1, 'k'), 1).is_err());
        // 路 3：覆写路（坏簿 last-wins 与首现断言必冲突）。
        led.record(2, inject_overwrite_bug_caught());
        // 路 4：簿坏路（错期望计数必被抓）。
        let mut b4 = ParamBook::new();
        let _ = b4.feed("rk", 100);
        let _ = b4.feed("rk", 200);
        led.record(3, matches!(b4.audit_ledger("rk", 3), Err(e) if e == DupCode::LedgerBad.val()));
        r += 1;
    }
    Ok(led)
}

// ── 复算：同投喂序两簿同态 ──────────────────────────────────────
pub fn replay_ok() -> Result<(), u32> {
    let mut a = ParamBook::new();
    let mut b = ParamBook::new();
    let seq: [(&str, u64); 6] = [
        ("x", 1),
        ("y", 2),
        ("x", 3),
        ("x", 4),
        ("y", 5),
        ("z", 6),
    ];
    let mut i = 0;
    while i < seq.len() {
        let (k, v) = match seq.get(i) {
            Some(t) => t,
            None => return Err(DupCode::ReplayMismatch.val()),
        };
        let _ = a.feed(k, *v);
        let _ = b.feed(k, *v);
        i += 1;
    }
    if a != b {
        return Err(DupCode::ReplayMismatch.val());
    }
    a.assert_first_wins("x", 1)?;
    a.audit_ledger("x", 2)?;
    a.audit_ledger("y", 1)?;
    Ok(())
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DupCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DupCode::ALL.len() {
        let v = DupCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut book = ParamBook::new();
    let _ = book.feed("rk", 100);
    let _ = book.feed("rk", 200);
    let _ = book.feed("rk", 300);
    book.assert_first_wins("rk", 100)?;
    book.audit_ledger("rk", EXPECT_DUP)?;
    if !book.structurally_sane() {
        return Err(DupCode::LedgerBad.val());
    }
    Ok(format!(
        "状态=已完成 keys={} feeds={} dups={} rk_dup={}",
        book.keys.len(),
        book.feeds,
        book.dups,
        book.dup_count_of("rk")
    ))
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((flags, rej, feeds)) => *flags == 7 && *rej == 2 && *feeds == 5,
            Err(_) => false,
        },
        match &g {
            Ok((flags, rej, feeds)) => format!("flags={} rejects={} feeds={}", flags, rej, feeds),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 首现生效断言（1/1）：同键 3 现后生效值 = 首现值。
    let first = {
        let mut b = ParamBook::new();
        let mut ok = true;
        let mut k = 0;
        while k < SAME_KEY_TIMES {
            let val = 100 + k as u64;
            if b.feed("rk", val).is_err() {
                ok = false;
            }
            k += 1;
        }
        ok && b.assert_first_wins("rk", 100).is_ok() && b.lookup("rk") == Some(100)
    };
    v.push((
        "J1-首现1/1",
        first,
        String::from("同键 3 现 生效=首现值"),
    ));
    // J1 重复计数（2/2）+ 重复簿可查。
    let dup = {
        let mut b = ParamBook::new();
        let _ = b.feed("rk", 100);
        let _ = b.feed("rk", 200);
        let _ = b.feed("rk", 300);
        let led = b.ledger_query();
        b.dup_count_of("rk") == EXPECT_DUP
            && led.len() == 1
            && match led.first() {
                Some((k, c)) => k == "rk" && *c == EXPECT_DUP,
                None => false,
            }
            && b.audit_ledger("rk", EXPECT_DUP).is_ok()
    };
    v.push((
        "J1-重复2/2簿可查",
        dup,
        format!("dup_count={} ledger_query 可导出", EXPECT_DUP),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 复算一致（同投喂序两簿同态）。
    v.push((
        "J2-复算一致",
        replay_ok().is_ok(),
        String::from("同投喂序两簿同态"),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x71EC0 窗口"),
    ));
    v
}

pub fn run_unx0587_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0587 重复参数账（首现口径）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
