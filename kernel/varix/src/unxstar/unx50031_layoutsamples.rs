//! UNX-F50031 布局样本页（N-up ×5 档样本入账）（0x43000 段）
//!
//! 锚点：UNX-F50031-J1 样本哈希账可复跑；判据锚定 §AI-63 §3 判据主轴。
//!
//! 数据段：`SEG_BASE = 0x43000`，诊断码闭集 `0x43000 + 1..=12`。
//!
//! 本模块结构：
//! - 样本记录：`SamplePage`（样本号 / N-up 档 / 页数 / 格数 / 内容摘要）。
//!   摘要由 (样本号, 档位, 页数, 格数, 逐页号) 逐步混入，是**纯函数**；
//! - 哈希账：`SampleLedger` 逐条入账并维护链 `h_n = mix(h_{n-1}, digest_n)`，
//!   `replay()` 从样本重算链并逐环比对——这就是「可复跑」的机器定义；
//!   顺序被换、摘要被改、页数越界分别落到不同错误码；
//! - 样本页生成：`LEVELS = [1,2,4,6,9]` 五档，格数按 ceil(页数/格) 补白；
//! - fuzz：`Lcg`（确定性线性同余，常参 6364136223846793005/1442695040888963407）
//!   驱动 `fuzz_report()`：每轮随机页数/档位入账，跑三项不变量（摘要重算
//!   一致、链可复跑、审计通过），再注入三类坏样本（摘要翻转、顺序交换、
//!   页数置零），逐类断言被拒；`FuzzReport` 带 `tail` 便于逐位复现；
//! - 注入核：`injection_core()` 与 fuzz 注入同源（共用 `audit_samples`
//!   与 `replay_pair`），不是两套判据。
//!
//! 零 panic 纪律：无裸下标、不做空解包；`Lcg` 用 wrapping 运算；无 std。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x43000 段，独占 0x20）。
pub const SEG_BASE: u32 = 0x43000;

/// 样本 N-up 五档（卡面口径：×5 档）。
pub const LEVELS: [u8; 5] = [1, 2, 4, 6, 9];
/// 单样本页数上限：样本页只验版面语义，超出 512 页没有额外覆盖价值。
pub const MAX_PAGES: u16 = 512;
/// 链起点盐：与摘要盐分离，避免「空链头 == 空摘要」的混淆。
pub const CHAIN_SALT: u64 = 0x4300_0BEE_F00D_0001;

const E_EMPTY: u16 = 1;
const E_NUP: u16 = 2;
const E_PAGES: u16 = 3;
const E_DUP: u16 = 4;
const E_FULL: u16 = 5;
const E_DIGEST: u16 = 6;
const E_CHAIN: u16 = 7;
const E_ORDER: u16 = 8;
const E_GAP: u16 = 9;
const E_FUZZ: u16 = 10;
const E_LCG: u16 = 11;
const E_CODE: u16 = 12;

/// 样本域错误十二型，`code()` 恒为 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    EmptyLedger,
    BadNUp,
    BadPages,
    Duplicate,
    Full,
    DigestMismatch,
    ChainBroken,
    AuditOrder,
    SampleGap,
    FuzzBroken,
    LcgStuck,
    CodeDrift,
}

impl SampleError {
    fn ordinal(self) -> u16 {
        match self {
            SampleError::EmptyLedger => E_EMPTY,
            SampleError::BadNUp => E_NUP,
            SampleError::BadPages => E_PAGES,
            SampleError::Duplicate => E_DUP,
            SampleError::Full => E_FULL,
            SampleError::DigestMismatch => E_DIGEST,
            SampleError::ChainBroken => E_CHAIN,
            SampleError::AuditOrder => E_ORDER,
            SampleError::SampleGap => E_GAP,
            SampleError::FuzzBroken => E_FUZZ,
            SampleError::LcgStuck => E_LCG,
            SampleError::CodeDrift => E_CODE,
        }
    }

    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            SampleError::EmptyLedger => "sample ledger empty",
            SampleError::BadNUp => "n-up not in sample set",
            SampleError::BadPages => "page count outside 1..=512",
            SampleError::Duplicate => "sample id duplicated",
            SampleError::Full => "sample ledger at capacity",
            SampleError::DigestMismatch => "sample digest recompute mismatch",
            SampleError::ChainBroken => "hash chain link mismatch",
            SampleError::AuditOrder => "sample ids out of order",
            SampleError::SampleGap => "sample id gap",
            SampleError::FuzzBroken => "fuzz invariant violated",
            SampleError::LcgStuck => "lcg output degenerate",
            SampleError::CodeDrift => "diagnostic code outside segment",
        }
    }
}

/// 全错误表（判据验证码值闭集）。
pub const ALL_ERRORS: [SampleError; 12] = [
    SampleError::EmptyLedger,
    SampleError::BadNUp,
    SampleError::BadPages,
    SampleError::Duplicate,
    SampleError::Full,
    SampleError::DigestMismatch,
    SampleError::ChainBroken,
    SampleError::AuditOrder,
    SampleError::SampleGap,
    SampleError::FuzzBroken,
    SampleError::LcgStuck,
    SampleError::CodeDrift,
];

fn mix(h: u64, v: u64) -> u64 {
    (h ^ v).wrapping_mul(0x100_0000_01b3)
}

/// N-up 档 -> (列, 行)。与样本集一致：1/2/4/6/9 五档。
pub fn grid_of(n_up: u8) -> Result<(u16, u16), SampleError> {
    match n_up {
        1 => Ok((1, 1)),
        2 => Ok((2, 1)),
        4 => Ok((2, 2)),
        6 => Ok((3, 2)),
        9 => Ok((3, 3)),
        _ => Err(SampleError::BadNUp),
    }
}

/// 一页样本记录。`cells` 含补白（ceil 到整格），因此 blank = cells - pages。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplePage {
    pub id: u16,
    pub n_up: u8,
    pub pages: u16,
    pub cells: u16,
    pub digest: u64,
}

/// 样本摘要：逐页号参与混合，保证「页数不同但格数相同」也能区分。
pub fn sample_digest(id: u16, n_up: u8, pages: u16, cells: u16) -> u64 {
    let mut h = CHAIN_SALT;
    h = mix(h, id as u64);
    h = mix(h, n_up as u64);
    h = mix(h, pages as u64);
    h = mix(h, cells as u64);
    let mut p = 1u16;
    while p <= pages {
        h = mix(h, (p as u64) ^ ((id as u64) << 16));
        p = p.saturating_add(1);
    }
    h
}

impl SamplePage {
    /// 规范构造：页数/档位校验 + 格数补白 + 摘要。样本的**唯一**生成入口。
    pub fn compute(id: u16, n_up: u8, pages: u16) -> Result<SamplePage, SampleError> {
        if id == 0 {
            return Err(SampleError::SampleGap);
        }
        if pages == 0 || pages > MAX_PAGES {
            return Err(SampleError::BadPages);
        }
        let (cols, rows) = grid_of(n_up)?;
        let per = cols.saturating_mul(rows);
        if per == 0 {
            return Err(SampleError::BadNUp);
        }
        let sheets = (pages + per - 1) / per;
        let cells = sheets.saturating_mul(per);
        Ok(SamplePage {
            id,
            n_up,
            pages,
            cells,
            digest: sample_digest(id, n_up, pages, cells),
        })
    }

    /// 补白格数。
    pub fn blank(&self) -> u16 {
        self.cells.saturating_sub(self.pages)
    }

    /// 逐项重算摘要并比对（单项校验，账本审计的基本单元）。
    pub fn verify(&self) -> Result<(), SampleError> {
        let again = SamplePage::compute(self.id, self.n_up, self.pages)?;
        if again.cells != self.cells || again.digest != self.digest {
            return Err(SampleError::DigestMismatch);
        }
        Ok(())
    }
}

/// 审计一组样本（顺序 + 页数 + 格数 + 摘要）。样本账与 fuzz 注入共用。
pub fn audit_samples(samples: &[SamplePage]) -> Result<(), SampleError> {
    if samples.is_empty() {
        return Err(SampleError::EmptyLedger);
    }
    let mut i = 0usize;
    while i < samples.len() {
        let s = match samples.get(i) {
            Some(v) => v,
            None => return Err(SampleError::SampleGap),
        };
        if s.id as usize != i + 1 {
            return Err(SampleError::AuditOrder);
        }
        if s.pages == 0 || s.pages > MAX_PAGES {
            return Err(SampleError::BadPages);
        }
        let (cols, rows) = grid_of(s.n_up)?;
        let per = cols.saturating_mul(rows);
        if per == 0 {
            return Err(SampleError::BadNUp);
        }
        let want_cells = ((s.pages + per - 1) / per).saturating_mul(per);
        if s.cells != want_cells {
            return Err(SampleError::DigestMismatch);
        }
        if s.digest != sample_digest(s.id, s.n_up, s.pages, s.cells) {
            return Err(SampleError::DigestMismatch);
        }
        i += 1;
    }
    Ok(())
}

/// 交换两条样本（容量守卫 + 无裸下标；样本数不足时返回 false）。
pub fn swap_samples(v: &mut [SamplePage], a: usize, b: usize) -> bool {
    if a == b || a >= v.len() || b >= v.len() {
        return false;
    }
    let va = match v.get(a).copied() {
        Some(x) => x,
        None => return false,
    };
    let vb = match v.get(b).copied() {
        Some(x) => x,
        None => return false,
    };
    if let Some(slot) = v.get_mut(a) {
        *slot = vb;
    }
    if let Some(slot) = v.get_mut(b) {
        *slot = va;
    }
    true
}

/// 链复算：`h_n = mix(mix(h_{n-1}, digest_n), id_n)`。链长与样本数不符即断。
pub fn replay_pair(samples: &[SamplePage], chain: &[u64]) -> Result<u64, SampleError> {
    if samples.is_empty() || chain.len() != samples.len() {
        return Err(SampleError::ChainBroken);
    }
    let mut h = CHAIN_SALT;
    let mut i = 0usize;
    while i < samples.len() {
        let s = match samples.get(i) {
            Some(v) => v,
            None => return Err(SampleError::ChainBroken),
        };
        h = mix(h, s.digest);
        h = mix(h, s.id as u64);
        match chain.get(i) {
            Some(want) => {
                if *want != h {
                    return Err(SampleError::ChainBroken);
                }
            }
            None => return Err(SampleError::ChainBroken),
        }
        i += 1;
    }
    Ok(h)
}

/// 样本哈希账：入账即算摘要与链，重放即重算链。容量/重复显式拒绝。
#[derive(Clone, Debug)]
pub struct SampleLedger {
    pub samples: Vec<SamplePage>,
    pub chain: Vec<u64>,
    pub cap: usize,
}

impl SampleLedger {
    pub fn new(cap: usize) -> SampleLedger {
        SampleLedger {
            samples: Vec::new(),
            chain: Vec::new(),
            cap,
        }
    }

    /// 入账一条样本（档位, 页数），返回样本号。样本号由账本顺序分配。
    pub fn admit(&mut self, n_up: u8, pages: u16) -> Result<u16, SampleError> {
        if self.samples.len() >= self.cap {
            return Err(SampleError::Full);
        }
        let id = (self.samples.len() as u16).saturating_add(1);
        let page = SamplePage::compute(id, n_up, pages)?;
        let mut h = self.head();
        h = mix(h, page.digest);
        h = mix(h, page.id as u64);
        self.samples.push(page);
        self.chain.push(h);
        Ok(id)
    }

    pub fn head(&self) -> u64 {
        match self.chain.last() {
            Some(v) => *v,
            None => CHAIN_SALT,
        }
    }

    /// 重放：从样本重算链并逐环比对（可复跑判据的执行点）。
    pub fn replay(&self) -> Result<u64, SampleError> {
        replay_pair(&self.samples, &self.chain)
    }

    /// 全账审计：非空 -> 容量 -> 顺序/摘要 -> 链。链错与摘要错分码。
    pub fn audit(&self) -> Result<(), SampleError> {
        if self.samples.is_empty() {
            return Err(SampleError::EmptyLedger);
        }
        if self.samples.len() > self.cap {
            return Err(SampleError::Full);
        }
        audit_samples(&self.samples)?;
        replay_pair(&self.samples, &self.chain)?;
        Ok(())
    }

    /// 按样本号取记录。
    pub fn get(&self, id: u16) -> Option<&SamplePage> {
        if id == 0 {
            return None;
        }
        self.samples.get(id as usize - 1)
    }
}

// ---------------------------------------------------------------------------
// 确定性 LCG（fuzz 驱动）
// ---------------------------------------------------------------------------

/// 64 位线性同余：`state = state*a + c`，取高位 32 位。常参取自 Numerical
/// Recipes 的 LCG64 组合（a=6364136223846793005, c=1442695040888963407）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 播种：`|1` 保证不退化为 0 不动点（0 是 LCG 的必然陷阱）。
    pub const fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 半开区间 [lo, hi)；空区间直接返回 lo（不 panic）。
    pub fn next_range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u32() % (hi - lo)
    }
}

/// fuzz 汇总。`tail` 把每轮链头混入，用来逐位复现整轮 fuzz。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzReport {
    pub rounds: u32,
    pub admits: u32,
    pub clean_ok: bool,
    pub injected: u32,
    pub caught: u32,
    pub tail: u64,
}

/// fuzz 主循环：每轮 3 条入账 + 3 类注入。注入与干净路径走同一套
/// `audit_samples`/`replay_pair`——注入必须红，干净必须绿，否则 `clean_ok`
/// 或 `caught` 计数立刻对不上（不恒绿、不恒红）。
pub fn fuzz_report(seed: u64, rounds: u32) -> FuzzReport {
    let mut admits = 0u32;
    let mut clean_ok = true;
    let mut injected = 0u32;
    let mut caught = 0u32;
    let mut tail: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut r = 0u32;
    while r < rounds {
        let mut lcg = Lcg::new(seed ^ (r as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let pages = lcg.next_range(1, MAX_PAGES as u32 + 1) as u16;
        let li = lcg.next_range(0, LEVELS.len() as u32) as usize;
        let level = match LEVELS.get(li) {
            Some(v) => *v,
            None => 1,
        };
        let li2 = lcg.next_range(0, LEVELS.len() as u32) as usize;
        let level2 = match LEVELS.get(li2) {
            Some(v) => *v,
            None => 2,
        };
        let pages2 = lcg.next_range(1, MAX_PAGES as u32 + 1) as u16;
        let mut led = SampleLedger::new(8);
        let mut round_ok = true;
        round_ok = round_ok && led.admit(level, pages).is_ok();
        round_ok = round_ok && led.admit(level2, pages2).is_ok();
        round_ok = round_ok && led.admit(1, 1).is_ok();
        admits += 3;
        if round_ok {
            // 不变量 1：每条样本摘要可重算。
            let mut i = 0usize;
            while i < led.samples.len() {
                if let Some(sp) = led.samples.get(i) {
                    if sp.verify().is_err() {
                        round_ok = false;
                    }
                }
                i += 1;
            }
            // 不变量 2：链可复跑且与入账链一致。
            round_ok = round_ok && led.replay().is_ok() && led.audit().is_ok();
        } else {
            clean_ok = false;
        }
        if !round_ok {
            clean_ok = false;
        }
        // 注入 1：摘要翻转。
        injected += 1;
        if !led.samples.is_empty() {
            let mut bad = led.samples.clone();
            if let Some(slot) = bad.get_mut(0) {
                slot.digest ^= 0x40;
            }
            if audit_samples(&bad).err() == Some(SampleError::DigestMismatch) {
                caught += 1;
            }
        }
        // 注入 2：顺序交换（摘要都对，唯有链序对不上）。
        injected += 1;
        if led.samples.len() >= 2 {
            let mut bad = led.samples.clone();
            if swap_samples(&mut bad, 0, 1)
                && replay_pair(&bad, &led.chain).err() == Some(SampleError::ChainBroken)
            {
                caught += 1;
            }
        }
        // 注入 3：页数置零。
        injected += 1;
        let mut bad = led.samples.clone();
        if let Some(slot) = bad.get_mut(0) {
            slot.pages = 0;
        }
        if audit_samples(&bad).err() == Some(SampleError::BadPages) {
            caught += 1;
        }
        tail = mix(tail, led.head());
        tail = mix(tail, pages as u64 ^ ((pages2 as u64) << 20));
        r += 1;
    }
    FuzzReport {
        rounds,
        admits,
        clean_ok,
        injected,
        caught,
        tail,
    }
}

// ---------------------------------------------------------------------------
// 注入核：三类坏样本（与 fuzz 同源）
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let total = 5usize;
    let mut caught = 0usize;
    let mut led = SampleLedger::new(8);
    let mut ok_all = true;
    for (lvl, pg) in [(1u8, 1u16), (2, 3), (4, 7), (6, 13), (9, 20)].iter() {
        if led.admit(*lvl, *pg).is_err() {
            ok_all = false;
        }
    }
    if !ok_all || led.audit().is_err() {
        return (0, total);
    }
    // 1. 摘要翻转。
    let mut bad = led.samples.clone();
    if let Some(s) = bad.get_mut(1) {
        s.digest ^= 1;
    }
    if audit_samples(&bad).err() == Some(SampleError::DigestMismatch) {
        caught += 1;
    }
    // 2. 顺序交换。
    let mut bad = led.samples.clone();
    if swap_samples(&mut bad, 0, 2)
        && replay_pair(&bad, &led.chain).err() == Some(SampleError::ChainBroken)
    {
        caught += 1;
    }
    // 3. 页数置零。
    let mut bad = led.samples.clone();
    if let Some(s) = bad.get_mut(2) {
        s.pages = 0;
    }
    if audit_samples(&bad).err() == Some(SampleError::BadPages) {
        caught += 1;
    }
    // 4. 档位越界。
    let mut bad = led.samples.clone();
    if let Some(s) = bad.get_mut(0) {
        s.n_up = 7;
    }
    if audit_samples(&bad).err() == Some(SampleError::BadNUp) {
        caught += 1;
    }
    // 5. 链长与样本数不符（截断链）。
    if replay_pair(&led.samples, &led.chain[..4]).err() == Some(SampleError::ChainBroken) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50031_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50031");

    // J1-1 样本哈希账可复跑：同输入两本账，链逐环比对相等。
    let mut replay_ok = false;
    let mut det_txt = String::new();
    let build = |pages: &[u16]| -> Option<SampleLedger> {
        let mut led = SampleLedger::new(8);
        let mut i = 0usize;
        while i < LEVELS.len() {
            let lvl = match LEVELS.get(i) {
                Some(v) => *v,
                None => return None,
            };
            let pg = match pages.get(i) {
                Some(v) => *v,
                None => return None,
            };
            if led.admit(lvl, pg).is_err() {
                return None;
            }
            i += 1;
        }
        Some(led)
    };
    let pages5: [u16; 5] = [1, 3, 7, 13, 20];
    match (build(&pages5), build(&pages5)) {
        (Some(a), Some(b)) => {
            replay_ok = a.head() == b.head()
                && a.chain == b.chain
                && a.samples == b.samples
                && a.replay().is_ok()
                && a.audit().is_ok()
                && a.replay() == Ok(a.head());
            det_txt = format!("head={:016x}", a.head());
        }
        _ => {}
    }
    s.add_dyn("50031-J1-哈希账可复跑", replay_ok, &det_txt);

    // J1-2 ×5 档入账：档位表恰 5 档且互异，每条样本档位与入账一致。
    let mut lv_ok = LEVELS.len() == 5;
    let mut i = 0usize;
    while i < LEVELS.len() {
        if let Some(v) = LEVELS.get(i) {
            let mut j = i + 1;
            while j < LEVELS.len() {
                if LEVELS.get(j) == Some(v) {
                    lv_ok = false;
                }
                j += 1;
            }
        }
        i += 1;
    }
    let mut led5 = SampleLedger::new(8);
    let mut admit_ok = true;
    let mut i = 0usize;
    while i < LEVELS.len() {
        if let Some(v) = LEVELS.get(i) {
            if led5.admit(*v, 5).is_err() {
                admit_ok = false;
            }
        }
        i += 1;
    }
    let mut match_ok = admit_ok;
    let mut i = 0usize;
    while i < led5.samples.len() {
        if let (Some(s), Some(l)) = (led5.samples.get(i), LEVELS.get(i)) {
            if s.n_up != *l {
                match_ok = false;
            }
        }
        i += 1;
    }
    let ok = lv_ok && match_ok && led5.samples.len() == 5 && led5.audit().is_ok();
    s.add("50031-J1-五档入账", ok, if ok { "" } else { "档位表或入账漂移" });

    // J1-3 格数与补白：4-up 13 页 -> 16 格 3 白；6-up 13 页 -> 18 格 5 白。
    let mut cell_ok = false;
    if let (Ok(a), Ok(b), Ok(c)) = (
        SamplePage::compute(1, 4, 13),
        SamplePage::compute(2, 6, 13),
        SamplePage::compute(3, 9, 5),
    ) {
        cell_ok = a.cells == 16
            && a.blank() == 3
            && b.cells == 18
            && b.blank() == 5
            && c.cells == 9
            && c.blank() == 4;
    }
    s.add("50031-J1-格数补白", cell_ok, if cell_ok { "" } else { "格数/补白错误" });

    // J1-4 摘要确定且敏感：同参两次相等；页数或档位一变即不同。
    let mut dig_ok = false;
    if let (Ok(a), Ok(b), Ok(c)) = (
        SamplePage::compute(4, 4, 7),
        SamplePage::compute(4, 4, 7),
        SamplePage::compute(4, 4, 8),
    ) {
        if let Ok(d) = SamplePage::compute(4, 6, 7) {
            dig_ok = a.digest == b.digest && a.digest != c.digest && a.digest != d.digest;
        }
    }
    s.add("50031-J1-摘要确定敏感", dig_ok, if dig_ok { "" } else { "摘要不敏感/非确定" });

    // J1-5 链头随样本变化：追加一条后链头必然改变，且旧链前缀不变。
    let mut head_ok = false;
    let mut led = SampleLedger::new(8);
    if led.admit(4, 8).is_ok() {
        let h1 = led.head();
        if led.admit(4, 8).is_ok() {
            let h2 = led.head();
            head_ok = h1 != h2
                && led.chain.get(0) == Some(&h1)
                && led.replay().is_ok()
                && led.audit().is_ok();
        }
    }
    s.add("50031-J1-链头敏感", head_ok, if head_ok { "" } else { "链头未随样本变化" });

    // J1-6 摘要篡改必抓：审计返回 DigestMismatch（不是笼统失败）。
    let mut tamper_ok = false;
    if let Ok(p) = SamplePage::compute(1, 4, 7) {
        let mut bad = p;
        bad.digest ^= 0x8000_0000;
        tamper_ok = audit_samples(&[bad]).err() == Some(SampleError::DigestMismatch)
            && bad.verify().err() == Some(SampleError::DigestMismatch);
    }
    s.add("50031-J1-篡改必抓", tamper_ok, if tamper_ok { "" } else { "篡改未被抓" });

    // J1-7 顺序交换必抓：摘要全对，链序对不上 -> ChainBroken。
    let mut swap_ok = false;
    let mut led = SampleLedger::new(8);
    if led.admit(1, 2).is_ok() && led.admit(2, 2).is_ok() && led.admit(4, 2).is_ok() {
        let mut bad = led.samples.clone();
        if swap_samples(&mut bad, 0, 2) {
            swap_ok = ledger_replay(&bad, &led.chain) == Err(SampleError::ChainBroken);
        }
    }
    s.add("50031-J1-换序必抓", swap_ok, if swap_ok { "" } else { "换序未断链" });

    // J1-8 页数越界：0 与 513 都是 BadPages（不含半开区间误判）。
    let ok = SamplePage::compute(1, 4, 0).err() == Some(SampleError::BadPages)
        && SamplePage::compute(1, 4, MAX_PAGES + 1).err() == Some(SampleError::BadPages)
        && SamplePage::compute(1, 4, MAX_PAGES).is_ok();
    s.add("50031-J1-页数边界", ok, if ok { "" } else { "页数边界误判" });

    // J1-9 样本号乱序/缺口必抓：审计返回 AuditOrder。
    let mut order_ok = false;
    if let (Ok(a), Ok(b)) = (SamplePage::compute(1, 4, 5), SamplePage::compute(2, 4, 5)) {
        order_ok = audit_samples(&[b, a]).err() == Some(SampleError::AuditOrder);
    }
    s.add("50031-J1-乱序必抓", order_ok, if order_ok { "" } else { "乱序未抓" });

    // J1-10 容量满：cap=2 时第三笔 Full，前两笔不受影响。
    let mut led = SampleLedger::new(2);
    let cap_ok = led.admit(1, 1).is_ok()
        && led.admit(2, 2).is_ok()
        && led.admit(4, 3).err() == Some(SampleError::Full)
        && led.samples.len() == 2
        && led.audit().is_ok();
    s.add("50031-J1-容量满拒绝", cap_ok, if cap_ok { "" } else { "容量满未拒绝" });

    // J1-11 空账本：audit/replay 都拒绝且分码（EmptyLedger vs ChainBroken）。
    let empty = SampleLedger::new(4);
    let ok = empty.audit().err() == Some(SampleError::EmptyLedger)
        && empty.replay().err() == Some(SampleError::ChainBroken);
    s.add("50031-J1-空账本拒绝", ok, if ok { "" } else { "空账本未分码" });

    // J1-12 LCG 确定性：同种子 64 抽逐位一致，异种子不同，且不卡死。
    let mut a = Lcg::new(0x1234_5678_9abc_def0);
    let mut b = Lcg::new(0x1234_5678_9abc_def0);
    let mut c = Lcg::new(0x0fed_cba9_8765_4321);
    let mut same = 0usize;
    let mut distinct = 0usize;
    let mut diff = 0usize;
    let mut prev = 0u32;
    let mut i = 0usize;
    while i < 64 {
        let x = a.next_u32();
        let y = b.next_u32();
        let z = c.next_u32();
        if x == y {
            same += 1;
        }
        if x != z {
            diff += 1;
        }
        if i > 0 && x != prev {
            distinct += 1;
        }
        prev = x;
        i += 1;
    }
    let ok = same == 64 && distinct >= 60 && diff >= 60;
    let det = format!("same={} distinct={} diff={}", same, distinct, diff);
    s.add_dyn("50031-J1-LCG确定", ok, &det);

    // J1-13 fuzz 256 轮：干净路径全绿、三类注入逐轮全抓、两次跑逐位一致。
    let r1 = fuzz_report(0x5003_1000_0000_0001, 256);
    let r2 = fuzz_report(0x5003_1000_0000_0001, 256);
    let fuzz_ok = r1.rounds == 256
        && r1.admits == 768
        && r1.clean_ok
        && r1.injected == 768
        && r1.caught == r1.injected
        && r1 == r2;
    let det = format!("admits={} caught={}/{}", r1.admits, r1.caught, r1.injected);
    s.add_dyn("50031-J1-Fuzz全抓", fuzz_ok, &det);

    // J1-14 fuzz 非恒绿：同一审计入口对干净样本放行、对注入样本拒绝。
    let mut nonconst_ok = false;
    if let Ok(good) = SamplePage::compute(1, 4, 9) {
        let mut bad = good;
        bad.pages = 0;
        nonconst_ok = audit_samples(&[good]).is_ok()
            && audit_samples(&[bad]).err() == Some(SampleError::BadPages);
    }
    s.add("50031-J1-Fuzz非恒绿", nonconst_ok, if nonconst_ok { "" } else { "审计结论恒定" });

    // J1-15 错误码闭集：12 条，码值 = 基址 + 序号且唯一。
    let mut codes_ok = ALL_ERRORS.len() == 12;
    let mut seen = [false; 13];
    let mut i = 0usize;
    while i < ALL_ERRORS.len() {
        if let Some(e) = ALL_ERRORS.get(i) {
            let c = e.code();
            if c != SEG_BASE + (i as u32) + 1 {
                codes_ok = false;
            }
            let off = (c - SEG_BASE) as usize;
            if off < seen.len() {
                if seen[off] {
                    codes_ok = false;
                }
                seen[off] = true;
            } else {
                codes_ok = false;
            }
        }
        i += 1;
    }
    s.add("50031-J1-错误码闭集", codes_ok, if codes_ok { "" } else { "码值漂移" });

    // J1-16 注入核 5 类全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50031-J1-注入必抓", ok, &det);

    s
}

/// 账本外重放（判据对「被换序的样本」复用同一链路函数，避免两套实现）。
fn ledger_replay(samples: &[SamplePage], chain: &[u64]) -> Result<u64, SampleError> {
    replay_pair(samples, chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50031_checks_green() {
        let set = run_unx50031_checks();
        assert!(set.all_passed(), "UNX-F50031 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50031_chain_replays() {
        let mut led = SampleLedger::new(8);
        assert!(led.admit(4, 13).is_ok());
        assert!(led.admit(9, 20).is_ok());
        match led.replay() {
            Ok(h) => assert_eq!(h, led.head()),
            Err(e) => panic!("replay failed: {:?}", e.code()),
        }
    }
}
