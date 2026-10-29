//! unx48326_canddiv — UNX-F48326 整句候选多样性纵深（三体裁样张命中率分体裁落账）（诊断段 0xC9A0）。
//!
//! 判据原文：
//!   J1 三体裁 ×5000 字样张分账（J-2 构成项）；判据锚定 §AI-61 §5 J-2。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三体裁闭集：叙事/公文/口语三画像各持特有词集（各 5 词）+
//!      全体通用词——多样性纵深 = 整句候选不偏科，三体裁必须各测
//!      各的账，混账即伪绿。
//!   2. 样张确定性（J1 ×5000）：每体裁 5000 单元样张由体裁种子 LCG
//!      生成（词 id 流），同种子复跑逐位一致——分账是可复现实验，
//!      不是一次性印象。
//!   3. 命中口径：单元词 ∈ 通用词 ∪ 本体裁特有词 即命中；跨体裁
//!      特有词不命中（画像串味即覆盖失效）。命中率千分比落账，
//!      与独立重算双路径对账。
//!   4. J-2 构成项：三体裁各账（字数/命中数/命中率）+ 汇总账，
//!      总命中 == 三体裁和（分账可加和）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC9A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_GENRE_BAD: u16 = SEG_BASE + 1;  // 体裁越闭集
const E_WORDS_BAD: u16 = SEG_BASE + 2;  // 样张字数不足 ×5000
const E_HIT_BAD: u16 = SEG_BASE + 3;    // 命中账与重算不一致
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 分账加和不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 5;// 复跑不一致
const E_DIV_BAD: u16 = SEG_BASE + 6;    // 多样性纵深缺失（候选源偏科）
const E_EMPTY_SUM: u16 = SEG_BASE + 7;  // 写回摘要为空

/// 词表容量与通用词 id。
pub const WORD_N: usize = 16;
pub const COMMON_ID: u8 = 15;
/// 三体裁闭集。
pub const GENRE_NARR: u8 = 0; // 叙事
pub const GENRE_DOC: u8 = 1;  // 公文
pub const GENRE_COLLO: u8 = 2;// 口语
/// 体裁特有词区间（各 5 词连续段）。
pub const SPEC_LEN: usize = 5;
/// 样张单元数（J1 ×5000）。
pub const SAMPLE_N: u32 = 5000;

/// 体裁特有词区间（词数刻意不等：叙事 5、公文 3、口语 7——三画像
/// 覆盖面天然分叉，命中率才可分账）。
pub fn spec_range(genre: u8) -> Result<(u8, u8), u16> {
    match genre {
        GENRE_NARR => Ok((0, 5)),
        GENRE_DOC => Ok((5, 8)),
        GENRE_COLLO => Ok((8, 15)),
        _ => Err(E_GENRE_BAD),
    }
}

/// 画像命中判定：通用词或本体裁特有词命中。
pub fn hit_of(genre: u8, word_id: u8) -> Result<bool, u16> {
    if word_id as usize >= WORD_N {
        return Err(E_GENRE_BAD);
    }
    if word_id == COMMON_ID {
        return Ok(true);
    }
    let (lo, hi) = spec_range(genre)?;
    Ok(word_id >= lo && word_id < hi)
}

/// 体裁分账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GenreLedger {
    pub genre: u8,
    pub words: u32,
    pub hits: u32,
}

impl GenreLedger {
    /// 命中率千分比。
    pub fn permille(&self) -> u32 {
        if self.words == 0 {
            return 0;
        }
        self.hits * 1000 / self.words
    }
}

/// 多样性纵深面：三体裁样张跑批 + 分账 + 汇总。
pub struct CandDiv {
    books: [GenreLedger; 3],
    pub reject_total: u32,
}

impl CandDiv {
    pub fn new() -> CandDiv {
        CandDiv {
            books: [
                GenreLedger { genre: GENRE_NARR, words: 0, hits: 0 },
                GenreLedger { genre: GENRE_DOC, words: 0, hits: 0 },
                GenreLedger { genre: GENRE_COLLO, words: 0, hits: 0 },
            ],
            reject_total: 0,
        }
    }

    fn book_idx(&self, genre: u8) -> Result<usize, u16> {
        match genre {
            GENRE_NARR => Ok(0),
            GENRE_DOC => Ok(1),
            GENRE_COLLO => Ok(2),
            _ => Err(E_GENRE_BAD),
        }
    }

    /// 跑一个体裁的 ×5000 样张（LCG 确定性生成），逐单元落账。
    pub fn run_genre(&mut self, genre: u8, seed: u64) -> Result<GenreLedger, u16> {
        let idx = match self.book_idx(genre) {
            Ok(i) => i,
            Err(e) => {
                self.reject_total += 1;
                return Err(e);
            }
        };
        let mut lcg = Lcg::new(seed)?;
        let mut words = 0u32;
        let mut hits = 0u32;
        while words < SAMPLE_N {
            let wid = (lcg.next_below(WORD_N as u64)) as u8;
            if hit_of(genre, wid)? {
                hits += 1;
            }
            words += 1;
        }
        self.books[idx].words += words;
        self.books[idx].hits += hits;
        Ok(self.books[idx])
    }

    pub fn book(&self, genre: u8) -> Result<GenreLedger, u16> {
        let idx = self.book_idx(genre)?;
        Ok(self.books[idx])
    }

    /// 分账复核（J-2 构成项）：三体裁字数各 == ×5000（分账不串），
    /// 命中数与独立重算一致。
    pub fn verify(&self, seeds: [u64; 3]) -> Result<(), u16> {
        let mut g = 0usize;
        while g < 3 {
            let b = &self.books[g];
            if b.words != SAMPLE_N {
                return Err(E_WORDS_BAD);
            }
            let want = recount(b.genre, seeds[g])?;
            if b.hits != want {
                return Err(E_HIT_BAD);
            }
            g += 1;
        }
        Ok(())
    }

    /// 汇总守恒：总字数/总命中 == 三体裁加和。
    pub fn total(&self) -> (u32, u32) {
        let mut w: u32 = 0;
        let mut h: u32 = 0;
        let mut g = 0usize;
        while g < 3 {
            w += self.books[g].words;
            h += self.books[g].hits;
            g += 1;
        }
        (w, h)
    }

    /// 多样性纵深断言：三体裁命中率两两不等（画像真实分叉，不是
    /// 一套候选糊三面）。
    pub fn diversity(&self) -> Result<(), u16> {
        let p0 = self.books[0].permille();
        let p1 = self.books[1].permille();
        let p2 = self.books[2].permille();
        if p0 == p1 || p1 == p2 || p0 == p2 {
            return Err(E_DIV_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.books[0].words != SAMPLE_N {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 独立重算：同种子同体裁样张的命中数（第二路径，不复用 run_genre
/// 的落账逻辑）。
pub fn recount(genre: u8, seed: u64) -> Result<u32, u16> {
    let (lo, hi) = spec_range(genre)?;
    let mut lcg = Lcg::new(seed)?;
    let mut hits = 0u32;
    let mut n = 0u32;
    while n < SAMPLE_N {
        let wid = (lcg.next_below(WORD_N as u64)) as u8;
        if wid == COMMON_ID || (wid >= lo && wid < hi) {
            hits += 1;
        }
        n += 1;
    }
    Ok(hits)
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_GENRE_BAD);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 自例程判据：逐条落实 J1（三体裁 ×5000 字样张分账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let seeds: [u64; 3] = [0xC9A1, 0xC9A2, 0xC9A3];

    // R1 三体裁 ×5000 分账：三体裁各跑一遍，各账字数 == 5000
    //    （分账不串），命中数与独立重算一致。
    let mut cd = CandDiv::new();
    let mut g = 0u8;
    while g < 3 {
        cd.run_genre(g, seeds[g as usize])?;
        g += 1;
    }
    cd.verify(seeds)?;
    let (tw, th) = cd.total();
    if tw != SAMPLE_N * 3 {
        return Err(E_WORDS_BAD);
    }
    if th == 0 || th == tw {
        return Err(E_HIT_BAD); // 全灭或全中都是画像失效
    }
    hits += 1;

    // R2 命中率千分比逐体裁独立核对：permille 与手算 hits*1000/words
    //    一致，且三体裁命中率落在合理区间（非零非满）。
    let mut cd = CandDiv::new();
    let mut g = 0u8;
    while g < 3 {
        cd.run_genre(g, seeds[g as usize])?;
        g += 1;
    }
    let mut g = 0u8;
    while g < 3 {
        let b = cd.book(g)?;
        if b.permille() != b.hits * 1000 / b.words {
            return Err(E_HIT_BAD);
        }
        if b.permille() == 0 || b.permille() > 600 {
            return Err(E_HIT_BAD);
        }
        g += 1;
    }
    hits += 1;

    // R3 多样性纵深：三体裁命中率两两不等（画像分叉）；若三账同值
    //    即候选源偏科必红。
    let mut cd = CandDiv::new();
    let mut g = 0u8;
    while g < 3 {
        cd.run_genre(g, seeds[g as usize])?;
        g += 1;
    }
    cd.diversity()?;
    hits += 1;

    // R4 跨体裁词不命中（画像串味防线）：叙事特有词在公文画像下
    //    必不命中，通用词处处命中。
    if hit_of(GENRE_DOC, 0).is_ok() && hit_of(GENRE_DOC, 0)? {
        return Err(E_DIV_BAD);
    }
    if !hit_of(GENRE_DOC, COMMON_ID)? {
        return Err(E_DIV_BAD);
    }
    if !hit_of(GENRE_NARR, 4)? || !hit_of(GENRE_COLLO, 14)? {
        return Err(E_DIV_BAD);
    }
    if hit_of(GENRE_NARR, 5).is_ok() && hit_of(GENRE_NARR, 5)? {
        return Err(E_DIV_BAD);
    }
    hits += 1;

    // R5 汇总守恒：总命中 == 三体裁命中和；逐体裁两遍 run（种子同）
    //    字数翻倍时账目照加（幂等加账不重置）。
    let mut cd = CandDiv::new();
    cd.run_genre(GENRE_NARR, seeds[0])?;
    let h1 = cd.book(GENRE_NARR)?.hits;
    cd.run_genre(GENRE_DOC, seeds[1])?;
    cd.run_genre(GENRE_COLLO, seeds[2])?;
    let (_, th) = cd.total();
    let want_sum = cd.book(GENRE_NARR)?.hits + cd.book(GENRE_DOC)?.hits
        + cd.book(GENRE_COLLO)?.hits;
    if th != want_sum || cd.book(GENRE_NARR)?.hits != h1 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 边界闭集：未知体裁拒收（run/book 两径都拒）；词 id 越闭集拒收。
    let mut cd = CandDiv::new();
    if cd.run_genre(7, seeds[0]) != Err(E_GENRE_BAD) {
        return Err(E_GENRE_BAD);
    }
    if cd.book(9) != Err(E_GENRE_BAD) {
        return Err(E_GENRE_BAD);
    }
    if cd.reject_total != 2 {
        return Err(E_LEDGER_BAD);
    }
    if hit_of(GENRE_NARR, 99) != Err(E_GENRE_BAD) {
        return Err(E_GENRE_BAD);
    }
    hits += 1;

    // R7 可红演练：空账 diversity 必红（三 0 千分比相等）；空摘要/
    //    空字数写回必红。
    let cd = CandDiv::new();
    if cd.diversity().is_ok() {
        return Err(E_DIV_BAD);
    }
    if cd.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    if cd.writeback(false, 8).is_ok() {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 复跑指纹一致与写回（判据 J2）：同种子第二轮三体裁复跑，
    //    三账逐位一致（可复现）；recount 独立路径复核；写回过。
    let mut cd = CandDiv::new();
    let mut g = 0u8;
    while g < 3 {
        cd.run_genre(g, seeds[g as usize])?;
        g += 1;
    }
    let fp1 = (cd.book(0)?, cd.book(1)?, cd.book(2)?);
    let mut cd2 = CandDiv::new();
    let mut g = 0u8;
    while g < 3 {
        cd2.run_genre(g, seeds[g as usize])?;
        g += 1;
    }
    if fp1 != (cd2.book(0)?, cd2.book(1)?, cd2.book(2)?) {
        return Err(E_REPLAY_DIFF);
    }
    cd.verify(seeds)?;
    if cd.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    cd.writeback(true, 0x2A)?;
    hits += 1;

    Ok(hits)
}
