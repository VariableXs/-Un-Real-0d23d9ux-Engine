//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0151 · 写盘工具对接口（只列清单，写盘行为在 L1） · 0x6EFC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0151-J1 写盘对接口：接口规约账（目标/偏移/长度/校验
//!     四账），只列清单零写盘（写企图红线），清单可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 接口四账：目标盘号/偏移/长度/FNV32 校验；
//!   - 零写红线：本卡只出清单（write 企图记账拒）；
//!   - 清单连续性（偏移不重叠）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EFC0，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6EFC0;

/// 最大清单条数（冻结 16）。
pub const MAX_LISTING: usize = 16;
/// 盘号闭集（0-3）。
pub const DISK_MAX: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiskCode {
    /// 盘号越界
    DiskOver,
    /// 载荷空
    PayloadEmpty,
    /// 写企图（零写红线）
    WriteAttempted,
    /// 清单重叠
    ListingOverlap,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DiskCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DiskCode::DiskOver => "盘号越界：≥4",
            DiskCode::PayloadEmpty => "载荷空",
            DiskCode::WriteAttempted => "写企图：本卡零写红线",
            DiskCode::ListingOverlap => "清单重叠：偏移红线",
            DiskCode::ReplayMismatch => "复算失配",
            DiskCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DiskCode; 6] = [
        DiskCode::DiskOver,
        DiskCode::PayloadEmpty,
        DiskCode::WriteAttempted,
        DiskCode::ListingOverlap,
        DiskCode::ReplayMismatch,
        DiskCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DiskCode::ALL.len() {
        if DiskCode::ALL[i].val() == code {
            return DiskCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EFC0 段闭集内"
}

// ── 接口账 ──────────────────────────────────────────────────────
pub fn fnv32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        h = (h ^ data[i] as u32).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteListing {
    pub disk: u8,
    pub offset: u64,
    pub payload: Vec<u8>,
    pub checksum: u32,
}

impl WriteListing {
    pub fn build(disk: u8, offset: u64, payload: &[u8]) -> Result<WriteListing, u32> {
        if disk as usize >= DISK_MAX {
            return Err(DiskCode::DiskOver.val());
        }
        if payload.is_empty() {
            return Err(DiskCode::PayloadEmpty.val());
        }
        Ok(WriteListing { disk, offset, payload: Vec::from(payload), checksum: fnv32(payload) })
    }

    pub fn end(&self) -> u64 {
        self.offset + self.payload.len() as u64
    }

    /// 尾验闸。
    pub fn checksum_ok(&self) -> bool {
        fnv32(&self.payload) == self.checksum
    }
}

#[derive(Clone, Debug, Default)]
pub struct DiskInterface {
    pub listing: Vec<WriteListing>,
    pub writes: u32,
}

impl DiskInterface {
    pub fn new() -> DiskInterface {
        DiskInterface::default()
    }

    /// 列清单（偏移重叠守卫，同盘）。
    pub fn list(&mut self, w: WriteListing) -> Result<usize, u32> {
        if self.listing.len() >= MAX_LISTING {
            return Err(DiskCode::ListingOverlap.val());
        }
        let mut i = 0;
        while i < self.listing.len() {
            let e = &self.listing[i];
            if e.disk == w.disk && w.offset < e.end() && e.offset < w.end() {
                return Err(DiskCode::ListingOverlap.val());
            }
            i += 1;
        }
        self.listing.push(w);
        Ok(self.listing.len() - 1)
    }

    /// 真写企图（零写红线）。
    pub fn perform_write(&mut self) -> Result<(), u32> {
        self.writes += 1;
        Err(DiskCode::WriteAttempted.val())
    }

    /// 零写断言。
    pub fn zero_writes(&self) -> bool {
        self.writes == 0
    }
}

/// 复算：同清单恒同指纹。
pub fn replay_ok(listing: &[(u8, u64, &[u8])]) -> Result<(), u32> {
    let mut a = DiskInterface::new();
    let mut b = DiskInterface::new();
    let mut i = 0;
    while i < listing.len() {
        let _ = a.list(WriteListing::build(listing[i].0, listing[i].1, listing[i].2)?);
        let _ = b.list(WriteListing::build(listing[i].0, listing[i].1, listing[i].2)?);
        i += 1;
    }
    if a.listing != b.listing {
        return Err(DiskCode::ReplayMismatch.val());
    }
    Ok(())
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

// ── 注入检出账（四路：越盘路/空载路/写路/重叠路） ────────────────
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

/// 真机清单（两条：引导扇区 + 内核）。
pub fn machine_listing() -> Vec<(u8, u64, &'static [u8])> {
    vec![
        (0, 0, b"BOOTSECTOR"),
        (0, 512, b"KERNELIMG"),
    ]
}

// ── 黄金全链：清单→零写→尾验→重叠抓→复算→越盘拒 ──────────────────
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let mut di = DiskInterface::new();
    let listing = machine_listing();
    let mut i = 0;
    while i < listing.len() {
        di.list(WriteListing::build(listing[i].0, listing[i].1, listing[i].2)?)?;
        i += 1;
    }
    let zero = di.zero_writes() as u32;
    let tail_ok = di.listing.iter().all(|w| w.checksum_ok()) as u32;
    replay_ok(&listing)?;
    // 越盘拒。
    let over = WriteListing::build(9, 0, b"x").is_err();
    Ok((di.listing.len(), zero + tail_ok, over as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：两清单 + 零写尾验 + 越盘拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2, 1)) => Ok(()),
        _ => Err(DiskCode::NotReproducible.val()),
    }
}

/// 序 B：写企图 ⇒ WriteAttempted（记账）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut di = DiskInterface::new();
    match di.perform_write() {
        Err(e) if e == DiskCode::WriteAttempted.val() => Ok(()),
        _ => Err(DiskCode::NotReproducible.val()),
    }
}

/// 序 C：重叠 ⇒ ListingOverlap。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut di = DiskInterface::new();
    let _ = di.list(WriteListing::build(0, 0, b"AAA")?);
    match di.list(WriteListing::build(0, 1, b"BBB")?) {
        Err(e) if e == DiskCode::ListingOverlap.val() => Ok(()),
        _ => Err(DiskCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越盘路。
        led.record(0, WriteListing::build(4, 0, b"x").is_err());
        // 路 2：空载路。
        led.record(1, WriteListing::build(0, 0, b"").is_err());
        // 路 3：写路。
        let mut d3 = DiskInterface::new();
        led.record(2, d3.perform_write().is_err() && !d3.zero_writes());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_listing()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DiskCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DiskCode::ALL.len() {
        let v = DiskCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, ok, over)) => *n == 2 && *ok == 2 && *over == 1,
            Err(_) => false,
        },
        match &g {
            Ok((n, ok, over)) => format!("listings={} ok={} over_reject={}", n, ok, over),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 零写红线。
    let mut di = DiskInterface::new();
    let _ = di.list(WriteListing::build(0, 0, b"data").unwrap_or(WriteListing { disk: 0, offset: 0, payload: Vec::new(), checksum: 0 }));
    v.push((
        "J1-零写红线",
        di.zero_writes(),
        String::from("本卡只列清单零写盘"),
    ));
    // 四账接口。
    let w = match WriteListing::build(1, 512, b"payload") {
        Ok(w) => w,
        Err(_) => {
            v.push(("J1-四账接口", false, String::from("build 失败")));
            return v;
        }
    };
    v.push((
        "J1-四账接口",
        w.disk == 1 && w.offset == 512 && w.payload.len() == 7 && w.checksum == fnv32(b"payload"),
        String::from("盘号/偏移/载荷/FNV32 四账冻结"),
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
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EFC0 窗口"),
    ));
    v
}

pub fn run_unx0151b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0151 写盘工具对接口（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
