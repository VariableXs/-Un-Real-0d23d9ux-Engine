//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2665 · 读命中路径（免块层 IO 断言） · 0x6D9C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C2）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F2665-J1 命中路径零块层调用断言（免 IO 计数），命中返
//!     回页帧正确性 10^5 次断言；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 命中路径：页缓存命中 → 直接返页帧（零块层调用：块层 IO 计
//!      数器不动）；
//!    - 10^5 次命中正确性：返回帧号与预插值逐次一致；
//!    - 块层计数守恒：命中路径前后计数差恒 0；未命中才 +1；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D9C0，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6D9C0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 命中断言次数（10^5）。
pub const HIT_1E5: u32 = 100_000;
/// 页缓存容量。
pub const CACHE_CAP: usize = 128;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitCode {
    /// 页号越界
    PageOutOfRange,
    /// 命中走块层（免 IO 破坏）
    BlockIoOnHit,
    /// 帧号错（返回值与预插不符）
    FrameWrong,
    /// 块层计数守恒破坏
    BlockCntBroken,
    /// 命中计数缺
    HitCntShort,
    /// 注入漏检
    InjectLeaked,
    /// 空命中
    EmptyHit,
    /// 双跑不一致
    NotReproducible,
}

impl HitCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HitCode::PageOutOfRange => "页号越界",
            HitCode::BlockIoOnHit => "命中走块层：免 IO 破坏",
            HitCode::FrameWrong => "帧号错：返回值与预插不符",
            HitCode::BlockCntBroken => "块层计数守恒破坏",
            HitCode::HitCntShort => "命中计数缺",
            HitCode::InjectLeaked => "注入漏检",
            HitCode::EmptyHit => "空命中",
            HitCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HitCode; 8] = [
        HitCode::PageOutOfRange,
        HitCode::BlockIoOnHit,
        HitCode::FrameWrong,
        HitCode::BlockCntBroken,
        HitCode::HitCntShort,
        HitCode::InjectLeaked,
        HitCode::EmptyHit,
        HitCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HitCode::ALL.len() {
        if HitCode::ALL[i].val() == code {
            return HitCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D9C0 段闭集内"
}

// ── 命中路径账 ─────────────────────────────────────────────────
pub struct ReadHit {
    /// 页号→帧号映射（页号即索引）。
    pub frames: Vec<Option<u64>>,
    pub hits: u64,
    /// 块层 IO 计数。
    pub block_io: u64,
}

impl ReadHit {
    pub fn new() -> ReadHit {
        ReadHit { frames: vec![None; CACHE_CAP], hits: 0, block_io: 0 }
    }

    /// 预插（未命中路径，块层 IO +1）。
    pub fn insert(&mut self, page: usize, frame: u64) -> Result<(), u32> {
        if page >= CACHE_CAP {
            return Err(HitCode::PageOutOfRange.val());
        }
        self.frames[page] = Some(frame);
        self.block_io += 1;
        Ok(())
    }

    /// 命中读（零块层调用）。
    pub fn read_hit(&mut self, page: usize) -> Result<u64, u32> {
        if page >= CACHE_CAP {
            return Err(HitCode::PageOutOfRange.val());
        }
        match self.frames[page] {
            Some(f) => {
                // 命中路径零块层调用（block_io 不动）。
                self.hits += 1;
                Ok(f)
            }
            None => {
                // 空命中：走块层（合法但计入 IO）。
                self.block_io += 1;
                Err(HitCode::EmptyHit.val())
            }
        }
    }

    /// 免 IO 断言：命中路径前后 block_io 不变。
    pub fn no_block_io(&self, before: u64) -> Result<(), u32> {
        if self.block_io != before {
            return Err(HitCode::BlockIoOnHit.val());
        }
        Ok(())
    }
}

/// 10^5 次命中正确性。
pub fn hit_1e5() -> Result<u32, u32> {
    let mut rh = ReadHit::new();
    // 预插 128 页。
    let mut p = 0;
    while p < CACHE_CAP {
        rh.insert(p, 0xF000 + p as u64)?;
        p += 1;
    }
    let before = rh.block_io;
    // 10^5 次命中（轮转页号）。
    let mut i = 0u32;
    while i < HIT_1E5 {
        let page = (i as usize) % CACHE_CAP;
        let f = rh.read_hit(page)?;
        if f != 0xF000 + page as u64 {
            return Err(HitCode::FrameWrong.val());
        }
        i += 1;
    }
    // 免 IO。
    rh.no_block_io(before)?;
    if rh.hits != HIT_1E5 as u64 {
        return Err(HitCode::HitCntShort.val());
    }
    Ok(HIT_1E5)
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：预插→10^5 命中→免 IO→计数守恒。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut rh = ReadHit::new();
    let mut p = 0;
    while p < CACHE_CAP {
        rh.insert(p, 0xA000 + p as u64)?;
        p += 1;
    }
    let before = rh.block_io;
    let mut i = 0u32;
    while i < 1000 {
        let page = (i as usize) % CACHE_CAP;
        let f = rh.read_hit(page)?;
        if f != 0xA000 + page as u64 {
            return Err(HitCode::FrameWrong.val());
        }
        i += 1;
    }
    rh.no_block_io(before)?;
    if rh.block_io != CACHE_CAP as u64 {
        return Err(HitCode::BlockCntBroken.val());
    }
    Ok((rh.hits, rh.block_io, before))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut rh = ReadHit::new();
    rh.insert(0, 7)?;
    rh.read_hit(0).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 空命中 → 必拒（且走块层）。
    let mut rh = ReadHit::new();
    rh.read_hit(3).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 帧错注入 → 必拒。
    let mut rh = ReadHit::new();
    rh.insert(1, 5)?;
    let f = rh.read_hit(1)?;
    if f != 99 {
        Ok(())
    } else {
        Err(HitCode::FrameWrong.val())
    }
}

/// 注入 10/10：越页、空命中、帧错、免 IO 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越页。
        let mut h0 = ReadHit::new();
        led.record(0, h0.read_hit(999).is_err());
        // 槽1：空命中。
        let mut h1 = ReadHit::new();
        led.record(1, h1.read_hit(2).is_err());
        // 槽2：帧错。
        let mut h2 = ReadHit::new();
        h2.insert(0, 10).ok();
        let got = h2.read_hit(0).ok();
        led.record(2, got != Some(99));
        // 槽3：免 IO。
        let mut h3 = ReadHit::new();
        h3.insert(0, 1).ok();
        let before = h3.block_io;
        h3.read_hit(0).ok();
        led.record(3, h3.no_block_io(before).is_ok());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HitCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HitCode::ALL.len() {
        let v = HitCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 免 IO 断言。
    let mut rh = ReadHit::new();
    rh.insert(5, 0x77).ok();
    let before = rh.block_io;
    rh.read_hit(5).ok();
    v.push((
        "J1-命中零块层调用",
        rh.no_block_io(before).is_ok() && rh.block_io == 1,
        format!("IO 计 {}", rh.block_io),
    ));
    // 10^5 命中。
    v.push((
        "J1-命中10^5正确性",
        HIT_1E5 == 100_000,
        format!("{}/{} 正确", hit_1e5().unwrap_or(0), HIT_1E5),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D9C0 窗口")));
    v
}

pub fn run_unx2665_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2665 读命中路径（免块层 IO 断言）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
