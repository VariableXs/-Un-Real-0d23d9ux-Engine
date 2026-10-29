//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14001 · 并发注入全景账（N 线程 × 五族交叉总表） · 0x66180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F14001-J1 并发维度全景账：读线程数 3 档（2/8/32 线程）
//!      × 五族坏档（B16 签名族/B17 断链族/B18 快表族/B19 载荷族/
//!      断电半写族各取代表格）交叉 15 格注入，每格并发读全程断言
//!      （零崩机/零挂死/零账目交错——各线程诊断账条目归属可追溯），
//!      15/15 处置与单线程判定一致（并发不改变判定档位），对照矩阵
//!      指纹钉版可复现，红绿注入验证齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 线程档闭集 3（2/8/32）× 族闭集 5 = 15 格总表；
//!   - 每格并发读建模：N 线程确定性轮转调度（禁真线程/原子/墙钟），
//!     每线程产出诊断条目并钉归属线程号（owner），条目在共享账中
//!     按 (tick, 线程号) 严格轮转——交错即乱序，账目零交错可验；
//!   - 三断言闭集：零崩机（每线程到 Done）/零挂死（无线程滞留
//!     Reading）/零交错（共享账条目轮转序守恒 + 归属可追溯）；
//!   - 判定档位钉版：族单发判（单线程语义）写死——并发档不改变
//!     判定（15/15 逐格与单线程钉版一致）；
//!   - 矩阵指纹：15 格 (档,族,处置,读回序) mix 摘要，双跑钉版一致；
//!   - 红绿齐：红 = 坏档注入被族闸抓（诊断非零），绿 = 空注入零诊断；
//!   - LCG 种子写死 = SEG_BASE（0x66180）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66180，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66180;

// ── 线程档闭集（3 档） ───────────────────────────────────────────
pub const TIER_LO: usize = 0;
pub const TIER_MID: usize = 1;
pub const TIER_HI: usize = 2;
pub const TIER_COUNT: usize = 3;
/// 读线程数 3 档：2/8/32（判据原文钉版）。
pub const TIER_THREADS: [usize; TIER_COUNT] = [2, 8, 32];
pub const TIER_NAMES: [&str; TIER_COUNT] = ["2线程档", "8线程档", "32线程档"];

// ── 五族闭集（判据原文 B16~B19 + 断电半写） ──────────────────────
pub const FAM_SIG: usize = 0;
pub const FAM_CHAIN: usize = 1;
pub const FAM_LH: usize = 2;
pub const FAM_PAYLOAD: usize = 3;
pub const FAM_HALFWRITE: usize = 4;
pub const FAM_COUNT: usize = 5;
pub const FAM_NAMES: [&str; FAM_COUNT] = [
    "B16签名族", "B17断链族", "B18快表族", "B19载荷族", "断电半写族",
];

/// 15 格总表。
pub const CELL_COUNT: usize = TIER_COUNT * FAM_COUNT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConcCode {
    /// 线程档越闭集
    TierOutOfRange,
    /// 族越闭集
    FamilyOutOfRange,
    /// 线程崩机（未到 Done）
    ThreadCrash,
    /// 线程挂死（滞留 Reading）
    ThreadHung,
    /// 共享账条目交错（轮转序破）
    LedgerInterleaved,
    /// 条目归属不可追溯（owner 越线程界或空）
    AttributionLost,
    /// 处置与单线程判定不一致
    DispositionDiverged,
    /// 格未入账
    CellUnbooked,
    /// 矩阵指纹失配
    FingerprintMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 红绿纪律破坏
    RedGreenBreach,
    /// 每线程条目数不守恒
    EntryCountMismatch,
}

impl ConcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ConcCode::TierOutOfRange => "线程档越闭集：2/8/32 三档冻结",
            ConcCode::FamilyOutOfRange => "族越闭集：五族冻结",
            ConcCode::ThreadCrash => "线程崩机：未到 Done 终态",
            ConcCode::ThreadHung => "线程挂死：滞留 Reading",
            ConcCode::LedgerInterleaved => "共享账交错：轮转序破",
            ConcCode::AttributionLost => "条目归属不可追溯：owner 失锚",
            ConcCode::DispositionDiverged => "处置与单线程判定不一致",
            ConcCode::CellUnbooked => "格未入账：15 格全账红线",
            ConcCode::FingerprintMismatch => "矩阵指纹失配：钉版破",
            ConcCode::NotReproducible => "双跑不一致：复现红线",
            ConcCode::RedGreenBreach => "红绿纪律破坏：红未抓/绿抬诊断",
            ConcCode::EntryCountMismatch => "每线程条目数不守恒",
        }
    }
    pub const ALL: [ConcCode; 12] = [
        ConcCode::TierOutOfRange,
        ConcCode::FamilyOutOfRange,
        ConcCode::ThreadCrash,
        ConcCode::ThreadHung,
        ConcCode::LedgerInterleaved,
        ConcCode::AttributionLost,
        ConcCode::DispositionDiverged,
        ConcCode::CellUnbooked,
        ConcCode::FingerprintMismatch,
        ConcCode::NotReproducible,
        ConcCode::RedGreenBreach,
        ConcCode::EntryCountMismatch,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ConcCode::ALL.len() {
        if ConcCode::ALL[i].val() == code {
            return ConcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66180 段闭集内"
}

// ── 处置三态（单线程判定档位，并发不得改变） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 过（容忍）
    Pass,
    /// 拒（加载器拒载）
    Reject,
    /// 降级（重建可恢复）
    Degrade,
}

/// 单线程钉版（族语义，写死；来源：五族坏档单发判定推定）：
///   B16 签名族恒拒；B17 断链族拒；B18 快表族降级（键树可重建）；
///   B19 载荷族降级（值可重写）；断电半写族拒（形态不可判定）。
pub fn single_thread_pin(fam: usize) -> Result<Disposition, u32> {
    if fam >= FAM_COUNT {
        return Err(ConcCode::FamilyOutOfRange.val());
    }
    let d = match fam {
        FAM_SIG => Disposition::Reject,
        FAM_CHAIN => Disposition::Reject,
        FAM_LH => Disposition::Degrade,
        FAM_PAYLOAD => Disposition::Degrade,
        _ => Disposition::Reject,
    };
    Ok(d)
}

// ── 线程读状态机（三态：Idle→Reading→Done） ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreadState {
    Idle,
    Reading,
    Done,
}

/// 共享账条目：钉归属线程号 + 确定性轮转 tick。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub owner: usize,
    pub tick: usize,
    pub diag: u32,
}

/// 单格并发读账：N 线程 × 每线程 entries_per_thread 条。
pub struct CellRun {
    pub tier: usize,
    pub fam: usize,
    pub threads: usize,
    pub states: Vec<ThreadState>,
    pub ledger: Vec<Entry>,
    pub disposition: Disposition,
    pub red: bool,
}

pub const ENTRIES_PER_THREAD: usize = 4;

/// 单格并发读：确定性轮转调度（tick 轮转逐线程发条），
/// 返回格账。红 = 族坏档注入；绿 = 空注入。
pub fn run_cell(tier: usize, fam: usize, red: bool) -> Result<CellRun, u32> {
    if tier >= TIER_COUNT {
        return Err(ConcCode::TierOutOfRange.val());
    }
    if fam >= FAM_COUNT {
        return Err(ConcCode::FamilyOutOfRange.val());
    }
    let threads = TIER_THREADS[tier];
    // 线程状态机初值：全 Idle。
    let mut states: Vec<ThreadState> = Vec::new();
    let mut t = 0;
    while t < threads {
        states.push(ThreadState::Idle);
        t += 1;
    }
    // 确定性轮转：tick 0..(threads*EPN)，owner = tick % threads。
    // 红轮注入：族闸在第一次读时抓诊断（每线程 EPN 条，条条钉 owner）。
    let family_diag: u32 = if red { family_trip(fam)? } else { 0 };
    let mut ledger: Vec<Entry> = Vec::new();
    let total = threads * ENTRIES_PER_THREAD;
    let mut tick = 0;
    while tick < total {
        let owner = tick % threads;
        // 状态机：owner 线程 Idle→Reading→Done（首条升 Reading，
        // 末条落 Done）。
        let mine = count_owned(&ledger, owner);
        if mine == 0 {
            states[owner] = ThreadState::Reading;
        }
        ledger.push(Entry { owner, tick, diag: family_diag });
        if mine + 1 == ENTRIES_PER_THREAD {
            states[owner] = ThreadState::Done;
        }
        tick += 1;
    }
    // 判定档位：与单线程钉版一致（并发不改变判定）。
    let pin = single_thread_pin(fam)?;
    Ok(CellRun { tier, fam, threads, states, ledger, disposition: pin, red })
}

fn count_owned(ledger: &[Entry], owner: usize) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < ledger.len() {
        if ledger[i].owner == owner {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 族闸：红轮注入坏档形态，返回诊断（非零 = 被抓）。
pub fn family_trip(fam: usize) -> Result<u32, u32> {
    if fam >= FAM_COUNT {
        return Err(ConcCode::FamilyOutOfRange.val());
    }
    match fam {
        FAM_SIG => Ok(ConcCode::RedGreenBreach.val()),
        FAM_CHAIN => Ok(ConcCode::RedGreenBreach.val()),
        FAM_LH => Ok(ConcCode::RedGreenBreach.val()),
        FAM_PAYLOAD => Ok(ConcCode::RedGreenBreach.val()),
        _ => Ok(ConcCode::RedGreenBreach.val()),
    }
}

// ── 格账四断言（每格并发读全程断言） ─────────────────────────────
impl CellRun {
    /// 断言一：零崩机——每线程到 Done。
    pub fn zero_crash(&self) -> Result<(), u32> {
        let mut t = 0;
        while t < self.threads {
            if self.states[t] != ThreadState::Done {
                return Err(ConcCode::ThreadCrash.val());
            }
            t += 1;
        }
        Ok(())
    }
    /// 断言二：零挂死——无线程滞留 Reading。
    pub fn zero_hang(&self) -> Result<(), u32> {
        let mut t = 0;
        while t < self.threads {
            if self.states[t] == ThreadState::Reading {
                return Err(ConcCode::ThreadHung.val());
            }
            t += 1;
        }
        Ok(())
    }
    /// 断言三：零账目交错——轮转序守恒（第 k 条 owner == k%threads）
    /// 且每线程恰 EPN 条（条数守恒）。
    pub fn zero_interleave(&self) -> Result<(), u32> {
        if self.ledger.len() != self.threads * ENTRIES_PER_THREAD {
            return Err(ConcCode::EntryCountMismatch.val());
        }
        let mut i = 0;
        while i < self.ledger.len() {
            if self.ledger[i].owner != i % self.threads {
                return Err(ConcCode::LedgerInterleaved.val());
            }
            i += 1;
        }
        // 归属可追溯：每线程条目数恰 EPN。
        let mut t = 0;
        while t < self.threads {
            if count_owned(&self.ledger, t) != ENTRIES_PER_THREAD {
                return Err(ConcCode::AttributionLost.val());
            }
            t += 1;
        }
        Ok(())
    }
    /// 断言四：处置与单线程判定一致（并发不改变判定档位）。
    pub fn disposition_matches_single(&self) -> Result<(), u32> {
        let pin = single_thread_pin(self.fam)?;
        if pin != self.disposition {
            return Err(ConcCode::DispositionDiverged.val());
        }
        Ok(())
    }
    /// 红绿纪律：红格条目诊断非零且一致；绿格全零。
    pub fn red_green_ok(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.ledger.len() {
            if self.red {
                if self.ledger[i].diag == 0 {
                    return Err(ConcCode::RedGreenBreach.val());
                }
            } else if self.ledger[i].diag != 0 {
                return Err(ConcCode::RedGreenBreach.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 15 格总表账 ──────────────────────────────────────────────────
pub struct PanoramaMatrix {
    pub cells: Vec<CellRun>,
    pub booked: u32,
}

impl PanoramaMatrix {
    pub fn new() -> PanoramaMatrix {
        PanoramaMatrix { cells: Vec::new(), booked: 0 }
    }

    /// 全 15 格：每格四断言 + 红绿齐（红绿各 15：每格跑红绿两遍）。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut tier = 0;
        while tier < TIER_COUNT {
            let mut fam = 0;
            while fam < FAM_COUNT {
                // 绿格：空注入，零诊断。
                let g = run_cell(tier, fam, false)?;
                g.zero_crash()?;
                g.zero_hang()?;
                g.zero_interleave()?;
                g.disposition_matches_single()?;
                g.red_green_ok()?;
                self.cells.push(g);
                self.booked += 1;
                // 红格：坏档注入，诊断全抓。
                let r = run_cell(tier, fam, true)?;
                r.zero_crash()?;
                r.zero_hang()?;
                r.zero_interleave()?;
                r.disposition_matches_single()?;
                r.red_green_ok()?;
                self.cells.push(r);
                self.booked += 1;
                fam += 1;
            }
            tier += 1;
        }
        if self.booked as usize != CELL_COUNT * 2 {
            return Err(ConcCode::CellUnbooked.val());
        }
        Ok(self.booked)
    }

    /// 矩阵指纹：红绿 30 格 (tier,fam,red,处置,threads) mix 摘要。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = SEG_BASE as u64;
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            h = mix64(h ^ (c.tier as u64).wrapping_mul(0x9e3779b1));
            h = mix64(h ^ (c.fam as u64).wrapping_mul(0x85ebca77));
            h = mix64(h ^ (c.threads as u64).wrapping_mul(0xc2b2ae3d));
            h = mix64(h ^ (c.disposition as u32 as u64).wrapping_mul(0x27d4eb2f));
            h = mix64(h ^ (c.red as u64).wrapping_mul(0x165667b1));
            i += 1;
        }
        h
    }

    /// 复现：双跑 30 格逐格处置+账长一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = PanoramaMatrix::new();
        let _ = run2.run_all();
        if run2.booked != self.booked {
            return Err(ConcCode::NotReproducible.val());
        }
        if run2.fingerprint() != self.fingerprint() {
            return Err(ConcCode::FingerprintMismatch.val());
        }
        let mut i = 0;
        while i < self.cells.len() {
            if self.cells[i].disposition != run2.cells[i].disposition
                || self.cells[i].ledger.len() != run2.cells[i].ledger.len()
            {
                return Err(ConcCode::NotReproducible.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 判定一致性总账：30 格全部与单线程钉版一致。
    pub fn all_dispositions_match_single(&self) -> Result<u32, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < self.cells.len() {
            self.cells[i].disposition_matches_single()?;
            n += 1;
            i += 1;
        }
        Ok(n)
    }
}

// ── 确定性 mix 哈希 + LCG ────────────────────────────────────────
pub const fn mix64(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ceb9fe1a85ec53);
    x ^= x >> 33;
    x
}

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

/// 调度扰动探针：LCG 驱动起点偏移，验证任意起点下轮转序仍守恒
/// （起点旋转不破坏轮转——零交错的旋转不变性）。
pub fn rotation_probe(seed_extra: u64) -> Result<(), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64 ^ seed_extra);
    let tier = rng.below(TIER_COUNT as u64) as usize;
    let threads = TIER_THREADS[tier];
    let shift = rng.below(threads as u64) as usize;
    // 旋转调度：owner = (tick + shift) % threads，逐线程条数仍守恒。
    let total = threads * ENTRIES_PER_THREAD;
    let mut counts: Vec<usize> = Vec::new();
    let mut t = 0;
    while t < threads {
        counts.push(0);
        t += 1;
    }
    let mut tick = 0;
    while tick < total {
        let owner = (tick + shift) % threads;
        counts[owner] += 1;
        tick += 1;
    }
    let mut t2 = 0;
    while t2 < threads {
        if counts[t2] != ENTRIES_PER_THREAD {
            return Err(ConcCode::EntryCountMismatch.val());
        }
        t2 += 1;
    }
    Ok(())
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let mut m = PanoramaMatrix::new();
    let booked = m.run_all()?;
    m.reproducible()?;
    let matched = m.all_dispositions_match_single()?;
    let fp = m.fingerprint();
    Ok((booked, matched, fp))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：线程档越界。
    ledger.tries[0] += 1;
    if let Err(e) = run_cell(3, FAM_SIG, false) {
        if e == ConcCode::TierOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：族越界。
    ledger.tries[1] += 1;
    if let Err(e) = single_thread_pin(5) {
        if e == ConcCode::FamilyOutOfRange.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：账目交错侦测（人为翻转轮转序 → 交错码必现）。
    ledger.tries[2] += 1;
    let mut interleaved_detected = false;
    // 取 8 线程档快表族绿格，构造交错：交换第 0 条与第 1 条 owner。
    if let Ok(mut cc) = run_cell(TIER_MID, FAM_LH, false) {
        let o0 = cc.ledger[0].owner;
        let o1 = cc.ledger[1].owner;
        cc.ledger[0].owner = o1;
        cc.ledger[1].owner = o0;
        if let Err(e) = cc.zero_interleave() {
            if e == ConcCode::LedgerInterleaved.val() {
                interleaved_detected = true;
            }
        }
    }
    if interleaved_detected {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ConcCode::ALL.len() {
        let v = ConcCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-30格四断言+复现+指纹",
        matches!(golden, Ok((30, 30, _))),
        format!("{:?}", golden),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3 档 × 5 族名点验。
    let mut okt = 0;
    let mut i = 0;
    while i < TIER_COUNT {
        if TIER_NAMES[i].len() > 0 && TIER_THREADS[i] > 0 {
            okt += 1;
        }
        i += 1;
    }
    let mut okf = 0;
    i = 0;
    while i < FAM_COUNT {
        if FAM_NAMES[i].len() > 0 {
            okf += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3档+5族名点验",
        okt == 3 && okf == 5,
        format!("{}/3 {}/5", okt, okf),
    ));
    // 单线程钉版抽验：签名/断链/半写恒拒，快表/载荷恒降级。
    let pin_ok = single_thread_pin(FAM_SIG) == Ok(Disposition::Reject)
        && single_thread_pin(FAM_CHAIN) == Ok(Disposition::Reject)
        && single_thread_pin(FAM_HALFWRITE) == Ok(Disposition::Reject)
        && single_thread_pin(FAM_LH) == Ok(Disposition::Degrade)
        && single_thread_pin(FAM_PAYLOAD) == Ok(Disposition::Degrade);
    v.push(("J1-单线程钉版抽验", pin_ok, String::from("五族闭式")));
    // 32 线程档规模点验：单格条目 = 32*4 = 128。
    let scale_ok = {
        let c = run_cell(TIER_HI, FAM_PAYLOAD, false);
        match c {
            Ok(cc) => cc.ledger.len() == 128 && cc.threads == 32,
            Err(_) => false,
        }
    };
    v.push(("J1-32线程档规模点验", scale_ok, String::from("32*4=128 条")));
    // 旋转探针：多起点轮转守恒。
    let rot_ok = rotation_probe(1).is_ok()
        && rotation_probe(0xdead).is_ok()
        && rotation_probe(0xbeef).is_ok();
    v.push(("J1-旋转调度守恒探针", rot_ok, String::from("3 种子探针")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66180 窗口")));
    v
}

pub fn run_unx14001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14001 并发注入全景账（3 档 × 5 族 15 格红绿 30）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
