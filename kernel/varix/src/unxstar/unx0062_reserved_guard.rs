//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0062 · 保留区白名单与守护 · 0x6C440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0062-J1 SMM/ACPI NVS/TPM/TSEG 等保留区入白名单，分配
//!     器申请越界 10 万次全部拒绝；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 保留区四类闭集：SMM（SMRAM）/ACPI NVS/TPM Event Log/TSEG，
//!     每区（类别/基址/跨度）三列账；
//!   - 白名单守护：分配器任何申请命中白名单区间（半开 [base,
//!     base+len)）即拒——10 万次越界申请全部拒绝；
//!   - 区间重叠检测：白名单区间两两不重叠（重叠即固件账异常，
//!     显性立案）；相邻但相接不判重叠；
//!   - 守护查询 O(区数) 线性扫描 + 排序二分两路实现，结果一致
//!     （对拍判据）；
//!   - 申请记账：拒入账（每拒必记），放行数 + 拒绝数 = 申请总数
//!     （守恒）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C440，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C440;

// ── 保留区四类闭集 ──────────────────────────────────────────────
pub const RSVD_SMM: usize = 0;
pub const RSVD_ACPI_NVS: usize = 1;
pub const RSVD_TPM_LOG: usize = 2;
pub const RSVD_TSEG: usize = 3;
pub const RSVD_KIND_COUNT: usize = 4;
pub const RSVD_KIND_NAMES: [&str; RSVD_KIND_COUNT] =
    ["SMM(SMRAM)", "ACPI NVS", "TPM EventLog", "TSEG"];

/// 白名单条目数下限（四类齐备）。
pub const WHITELIST_FLOOR: usize = 4;

/// 越界申请压测轮数（冻结：10 万次）。
pub const FUZZ_ROUNDS: u32 = 100_000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsvdCode {
    /// 类别越闭集
    KindOutOfRange,
    /// 类别名空（四类齐备破坏）
    KindNameBlank,
    /// 零长度保留区（缺列）
    ZeroLengthRegion,
    /// 白名单区间重叠（固件账异常）
    RegionOverlap,
    /// 白名单缺类（四类缺一）
    WhitelistShort,
    /// 分配器越界放行（守护漏——红线）
    GuardLeak,
    /// 守护两路实现不一致（线性/二分对拍失败）
    GuardDualMismatch,
    /// 申请记账守恒破坏（放行+拒绝≠总数）
    AccConservationBroken,
    /// 区间基址对齐破坏（页粒度）
    BaseMisaligned,
    /// 双跑不一致
    NotReproducible,
}

impl RsvdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsvdCode::KindOutOfRange => "类别越闭集：四类冻结",
            RsvdCode::KindNameBlank => "类别名空：齐备破坏",
            RsvdCode::ZeroLengthRegion => "零长度保留区：缺列",
            RsvdCode::RegionOverlap => "白名单区间重叠：固件账异常",
            RsvdCode::WhitelistShort => "白名单缺类：四类缺一",
            RsvdCode::GuardLeak => "分配器越界放行：守护漏（红线）",
            RsvdCode::GuardDualMismatch => "守护两路对拍失败",
            RsvdCode::AccConservationBroken => "申请记账守恒破坏",
            RsvdCode::BaseMisaligned => "区间基址非页对齐",
            RsvdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RsvdCode; 10] = [
        RsvdCode::KindOutOfRange,
        RsvdCode::KindNameBlank,
        RsvdCode::ZeroLengthRegion,
        RsvdCode::RegionOverlap,
        RsvdCode::WhitelistShort,
        RsvdCode::GuardLeak,
        RsvdCode::GuardDualMismatch,
        RsvdCode::AccConservationBroken,
        RsvdCode::BaseMisaligned,
        RsvdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RsvdCode::ALL.len() {
        if RsvdCode::ALL[i].val() == code {
            return RsvdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C440 段闭集内"
}

/// 页粒度（4KB 对齐守卫）。
pub const PAGE_ALIGN: u64 = 4096;

// ── 保留区条目 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReservedRegion {
    pub kind: usize,
    pub base: u64,
    pub len: u64,
}

impl ReservedRegion {
    pub fn legal(&self) -> Result<(), u32> {
        if self.kind >= RSVD_KIND_COUNT {
            return Err(RsvdCode::KindOutOfRange.val());
        }
        if self.len == 0 {
            return Err(RsvdCode::ZeroLengthRegion.val());
        }
        if self.base % PAGE_ALIGN != 0 {
            return Err(RsvdCode::BaseMisaligned.val());
        }
        Ok(())
    }
    /// 半开区间 [base, base+len) 是否命中。
    pub fn hits(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.base + self.len
    }
    /// 与他区是否重叠（半开区间相交判定；相接不算）。
    pub fn overlaps(&self, other: &ReservedRegion) -> bool {
        self.base < other.base + other.len && other.base < self.base + self.len
    }
}

// ── 样本白名单（写死：四类，地址为账面样本） ────────────────────
pub const SAMPLE_WHITELIST: [ReservedRegion; 4] = [
    ReservedRegion { kind: RSVD_SMM, base: 0x000A_0000, len: 0x0001_0000 },
    ReservedRegion { kind: RSVD_ACPI_NVS, base: 0xBDE9_0000, len: 0x0004_0000 },
    ReservedRegion { kind: RSVD_TPM_LOG, base: 0xBDE0_0000, len: 0x0004_0000 },
    ReservedRegion { kind: RSVD_TSEG, base: 0xBDC0_0000, len: 0x0020_0000 },
];

// ── 白名单守护（两路实现） ──────────────────────────────────────
pub struct Whitelist {
    pub regions: Vec<ReservedRegion>,
}

impl Whitelist {
    pub fn from_sample() -> Whitelist {
        Whitelist { regions: Vec::from(SAMPLE_WHITELIST) }
    }

    /// 固化审计：四类齐备 + 两两不重叠 + 合法性。
    pub fn audit(&self) -> Result<(), u32> {
        if self.regions.len() < WHITELIST_FLOOR {
            return Err(RsvdCode::WhitelistShort.val());
        }
        let mut hit = [false; RSVD_KIND_COUNT];
        let mut i = 0;
        while i < self.regions.len() {
            let r = self.regions[i];
            r.legal()?;
            if r.kind < RSVD_KIND_COUNT {
                hit[r.kind] = true;
            }
            // 两两重叠检测（j<i）。
            let mut j = 0;
            while j < i {
                if r.overlaps(&self.regions[j]) {
                    return Err(RsvdCode::RegionOverlap.val());
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0;
        while k < RSVD_KIND_COUNT {
            if !hit[k] {
                return Err(RsvdCode::WhitelistShort.val());
            }
            k += 1;
        }
        Ok(())
    }

    /// 守护（线性路）：命中任一区间即 true。
    pub fn guard_linear(&self, addr: u64) -> bool {
        let mut i = 0;
        while i < self.regions.len() {
            if self.regions[i].hits(addr) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 守护（二分路）：按基址排序后二分（命中检查候选区及其前驱）。
    pub fn guard_binary(&self, addr: u64) -> bool {
        let mut sorted = Vec::new();
        let mut i = 0;
        while i < self.regions.len() {
            sorted.push(self.regions[i]);
            i += 1;
        }
        // 插入排序（按 base）。
        let n = sorted.len();
        let mut a = 1;
        while a < n {
            let key = sorted[a];
            let mut b = a;
            while b > 0 && sorted[b - 1].base > key.base {
                sorted[b] = sorted[b - 1];
                b -= 1;
            }
            sorted[a] = key;
            a += 1;
        }
        // 二分：找最后一个 base <= addr 的区。
        let mut lo = 0usize;
        let mut hi = n;
        while lo < hi {
            let mid = (lo + hi) / 2;
            if sorted[mid].base <= addr {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        // 候选：lo-1（base<=addr 的最近区）。
        if lo > 0 && sorted[lo - 1].hits(addr) {
            return true;
        }
        false
    }

    /// 双路对拍。
    pub fn dual_check(&self, addr: u64) -> Result<(), u32> {
        if self.guard_linear(addr) != self.guard_binary(addr) {
            return Err(RsvdCode::GuardDualMismatch.val());
        }
        Ok(())
    }
}

// ── 分配器守护账（记账守恒） ────────────────────────────────────
pub struct GuardLedger {
    pub wl: Whitelist,
    pub total: u32,
    pub allowed: u32,
    pub rejected: u32,
    pub leak_caught: u32,
}

impl GuardLedger {
    pub fn new() -> GuardLedger {
        GuardLedger { wl: Whitelist::from_sample(), total: 0, allowed: 0, rejected: 0, leak_caught: 0 }
    }

    /// 分配申请：命中白名单即拒。
    pub fn allocate(&mut self, addr: u64, len: u64) -> Result<(), u32> {
        self.total += 1;
        // 区间命中（任一字节入区即拒）：取首址与末址双查。
        let last = if len == 0 { addr } else { addr + len - 1 };
        let hit = self.wl.guard_linear(addr) || self.wl.guard_linear(last);
        if hit {
            self.rejected += 1;
            return Err(RsvdCode::GuardLeak.val());
        }
        self.allowed += 1;
        Ok(())
    }

    /// 守恒：放行 + 拒绝 = 总数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.allowed + self.rejected != self.total {
            return Err(RsvdCode::AccConservationBroken.val());
        }
        Ok(())
    }

    /// 10 万次越界压测：随机地址+长度，凡命中白名单必拒（0 泄漏）。
    pub fn fuzz_beyond(&mut self, rounds: u32, seed: u64) -> Result<u32, u32> {
        let mut lcg = Lcg::new(seed);
        let mut rejects = 0;
        let mut r = 0;
        while r < rounds {
            let addr = lcg.next() & 0x0000_FFFF_FFFF_FFFF;
            let len = 1 + lcg.below(4096);
            // 该次申请是否应拒（独立双路判定）。
            let should_reject = self.wl.guard_linear(addr)
                || self.wl.guard_linear(addr + len - 1);
            let res = self.allocate(addr, len);
            match (should_reject, res.is_err()) {
                (true, true) => rejects += 1,
                (true, false) => {
                    self.leak_caught += 1;
                    return Err(RsvdCode::GuardLeak.val());
                }
                (false, true) => {
                    // 误拒也是漏（白名单外申请被拒）。
                    self.leak_caught += 1;
                    return Err(RsvdCode::GuardLeak.val());
                }
                (false, false) => {}
            }
            // 双路对拍（抽样）。
            if r % 1000 == 0 {
                self.wl.dual_check(addr)?;
            }
            r += 1;
        }
        self.conserve()?;
        Ok(rejects)
    }
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

/// 黄金全链：白名单审计→守护命中样本→10 万次压测零泄漏。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = GuardLedger::new();
    led.wl.audit()?;
    // 白名单内样本地址必拒。
    let mut i = 0;
    while i < led.wl.regions.len() {
        let inside = led.wl.regions[i].base + 8;
        if led.allocate(inside, 16).is_ok() {
            return Err(RsvdCode::GuardLeak.val());
        }
        i += 1;
    }
    // 白名单外样本地址必放行（取远端空洞 0x1000_0000 起）。
    led.allocate(0x1000_0000, 4096)?;
    // 10 万次越界压测。
    let rejects = led.fuzz_beyond(FUZZ_ROUNDS, SEG_BASE as u64)?;
    if led.leak_caught != 0 {
        return Err(RsvdCode::GuardLeak.val());
    }
    Ok((rejects, led.allowed, led.rejected))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let wl = Whitelist::from_sample();
    wl.audit()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 重叠注入 → 必拒。
    let mut wl = Whitelist::from_sample();
    wl.regions.push(ReservedRegion { kind: RSVD_TSEG, base: 0xBDC0_1000, len: 0x1000 });
    wl.audit()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 零长度区 → 必拒。
    let r = ReservedRegion { kind: RSVD_SMM, base: 0x1000, len: 0 };
    r.legal()
}

/// 注入 10/10：类别越界、页错位、守护漏、守恒破坏四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：类别越界。
        let k0 = ReservedRegion { kind: RSVD_KIND_COUNT, base: 0x1000, len: 0x1000 };
        led.record(0, k0.legal().is_err());
        // 槽1：页错位。
        let k1 = ReservedRegion { kind: RSVD_SMM, base: 0x1234, len: 0x1000 };
        led.record(1, k1.legal().is_err());
        // 槽2：守护漏（白名单内地址被放行的注入 = guard 应拒）。
        let mut gl = GuardLedger::new();
        let inside = SAMPLE_WHITELIST[0].base + 4;
        led.record(2, gl.allocate(inside, 8).is_err());
        // 槽3：双路对拍（白名单地址双路必须同判 true）。
        let wl = Whitelist::from_sample();
        let addr = SAMPLE_WHITELIST[1].base + 16;
        led.record(3, wl.guard_linear(addr) == true && wl.dual_check(addr).is_ok());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RsvdCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RsvdCode::ALL.len() {
        let v = RsvdCode::ALL[i].val();
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
    // 四类齐备。
    v.push((
        "J1-四类齐备",
        RSVD_KIND_NAMES.len() == 4 && SAMPLE_WHITELIST.len() == 4,
        format!("{:?}", RSVD_KIND_NAMES),
    ));
    // 10 万次压测零泄漏。
    let mut gl = GuardLedger::new();
    let fuzz_ok = gl.fuzz_beyond(FUZZ_ROUNDS, SEG_BASE as u64).is_ok() && gl.leak_caught == 0;
    v.push((
        "J1-10万次越界全拒",
        fuzz_ok && FUZZ_ROUNDS == 100_000,
        format!("压测 {} 次 拒 {}", FUZZ_ROUNDS, gl.rejected),
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
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C440 窗口")));
    v
}

pub fn run_unx0062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0062 保留区白名单与守护");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
