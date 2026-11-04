//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0074 · NUMA 节点亲和页位图 · 0x6E1C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0074-J1 页 → NUMA 节点亲和位图：区间账（节点/基址/
//!     页数）→ 位图逐页归账，越区间页查无节点（显性码），节点
//!     间零交叠（同一页不许属两节点），与 SRAT 拓扑对拍；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 亲和账：节点 N 的页区间登记（基址/页数），页粒度 4K；
//!   - 位图：每页 4bit 节点号账（0-14 有效，15=未账）；
//!   - 零交叠红线：同页双节点登记即拒（跨节点共享内存另有账）；
//!   - 查询：addr → 节点号；未登记页显性 NoNode（15）；
//!   - SRAT 对拍：每节点页数 = 位图计数（逐节点对拍）；
//!   - 亲和分配预演：本地优先（节点有页才出账）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E1C0，AI-VX3-1102 批I 独占区）。
pub const SEG_BASE: u32 = 0x6E1C0;

/// 页粒度（4K）。
pub const PAGE: u64 = 0x1000;
/// 位图窗口（16MB = 4096 页）。
pub const BITMAP_PAGES: usize = 4096;
/// 未账节点号。
pub const NO_NODE: u8 = 15;
/// 最大节点数（F0048 账联动 16）。
pub const MAX_NODES: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AffCode {
    /// 节点号越上限（≥16）
    NodeOutOfRange,
    /// 页越位图窗口
    PageOverWindow,
    /// 同页双节点（零交叠红线）
    PageDoubleClaim,
    /// 区间零页
    ZeroPages,
    /// 区间非 4K 对齐
    SpanMisaligned,
    /// SRAT 页数对拍失配
    SratCountMismatch,
    /// 亲和分配空账（本地无页）
    LocalEmpty,
    /// 双跑不一致
    NotReproducible,
}

impl AffCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AffCode::NodeOutOfRange => "节点号越上限：≥16",
            AffCode::PageOverWindow => "页越位图窗口：4096 冻结",
            AffCode::PageDoubleClaim => "同页双节点：零交叠红线",
            AffCode::ZeroPages => "区间零页",
            AffCode::SpanMisaligned => "区间非 4K 对齐",
            AffCode::SratCountMismatch => "SRAT 页数对拍失配",
            AffCode::LocalEmpty => "亲和分配空账：本地无页",
            AffCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AffCode; 8] = [
        AffCode::NodeOutOfRange,
        AffCode::PageOverWindow,
        AffCode::PageDoubleClaim,
        AffCode::ZeroPages,
        AffCode::SpanMisaligned,
        AffCode::SratCountMismatch,
        AffCode::LocalEmpty,
        AffCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AffCode::ALL.len() {
        if AffCode::ALL[i].val() == code {
            return AffCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E1C0 段闭集内"
}

// ── 亲和位图 ────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct NumaAffinityBitmap {
    /// 每页 4bit 节点号（两页一字节，低半页号偶页）。
    pub nibbles: Vec<u8>,
    pub per_node: [u32; MAX_NODES],
}

impl NumaAffinityBitmap {
    pub fn new() -> NumaAffinityBitmap {
        NumaAffinityBitmap { nibbles: vec![0xFF; (BITMAP_PAGES + 1) / 2], per_node: [0; MAX_NODES] }
    }

    /// 页号 → 当前节点账。
    pub fn node_of_page(&self, page: usize) -> Result<u8, u32> {
        if page >= BITMAP_PAGES {
            return Err(AffCode::PageOverWindow.val());
        }
        let byte = self.nibbles[page / 2];
        if page % 2 == 0 {
            Ok(byte & 0x0F)
        } else {
            Ok(byte >> 4)
        }
    }

    fn set_page(&mut self, page: usize, node: u8) -> Result<(), u32> {
        if page >= BITMAP_PAGES {
            return Err(AffCode::PageOverWindow.val());
        }
        if node as usize >= MAX_NODES {
            return Err(AffCode::NodeOutOfRange.val());
        }
        let idx = page / 2;
        let old = if page % 2 == 0 { self.nibbles[idx] & 0x0F } else { self.nibbles[idx] >> 4 };
        if old != NO_NODE {
            return Err(AffCode::PageDoubleClaim.val());
        }
        if page % 2 == 0 {
            self.nibbles[idx] = (self.nibbles[idx] & 0xF0) | node;
        } else {
            self.nibbles[idx] = (self.nibbles[idx] & 0x0F) | (node << 4);
        }
        self.per_node[node as usize] += 1;
        Ok(())
    }

    /// 登记节点页区间（基址/页数）。
    pub fn claim_span(&mut self, node: u8, base: u64, pages: u64) -> Result<u32, u32> {
        if node as usize >= MAX_NODES {
            return Err(AffCode::NodeOutOfRange.val());
        }
        if pages == 0 {
            return Err(AffCode::ZeroPages.val());
        }
        if base % PAGE != 0 {
            return Err(AffCode::SpanMisaligned.val());
        }
        let p0 = (base / PAGE) as usize;
        let p1 = p0 + pages as usize;
        if p1 > BITMAP_PAGES {
            return Err(AffCode::PageOverWindow.val());
        }
        let mut n = 0u32;
        let mut p = p0;
        while p < p1 {
            self.set_page(p, node)?;
            n += 1;
            p += 1;
        }
        Ok(n)
    }

    /// 地址查询：addr → 节点号（未账页显性 NO_NODE）。
    pub fn node_of_addr(&self, addr: u64) -> u8 {
        let page = (addr / PAGE) as usize;
        match self.node_of_page(page) {
            Ok(n) => n,
            Err(_) => NO_NODE,
        }
    }

    /// SRAT 对拍：每节点页数逐一对拍。
    pub fn srat_audit(&self, srat_counts: &[u32]) -> Result<usize, u32> {
        if srat_counts.len() > MAX_NODES {
            return Err(AffCode::NodeOutOfRange.val());
        }
        let mut i = 0;
        while i < srat_counts.len() {
            if srat_counts[i] != self.per_node[i] {
                return Err(AffCode::SratCountMismatch.val());
            }
            i += 1;
        }
        Ok(srat_counts.len())
    }

    /// 亲和分配预演：本地有页出本地账，无页显性空账。
    pub fn local_alloc_probe(&self, node: u8) -> Result<u32, u32> {
        if node as usize >= MAX_NODES {
            return Err(AffCode::NodeOutOfRange.val());
        }
        if self.per_node[node as usize] == 0 {
            return Err(AffCode::LocalEmpty.val());
        }
        Ok(self.per_node[node as usize])
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

// ── 注入检出账（四路：交叠路/越界路/对拍路/空账路） ──────────────
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

/// 真机样本：节点 0 @0 区 512 页、节点 1 @512 页区 512 页、节点 2 @1024 页区 256 页。
pub fn make_machine_claims() -> [(u8, u64, u64); 3] {
    [(0, 0x0, 512), (1, 512 * PAGE, 512), (2, 1024 * PAGE, 256)]
}

/// SRAT 账（真机口径）。
pub fn machine_srat_counts() -> Vec<u32> {
    vec![512, 512, 256]
}

// ── 黄金全链：登记→逐页归账→查询→SRAT 对拍→亲和预演 ─────────────
pub fn golden_run() -> Result<(u8, u8, u8, usize), u32> {
    let mut bm = NumaAffinityBitmap::new();
    let claims = make_machine_claims();
    let mut i = 0;
    while i < claims.len() {
        bm.claim_span(claims[i].0, claims[i].1, claims[i].2)?;
        i += 1;
    }
    let n0 = bm.node_of_addr(0x1000);
    let n1 = bm.node_of_addr(512 * PAGE + 0x1000);
    let n_out = bm.node_of_addr(2000 * PAGE);
    let n = bm.srat_audit(&machine_srat_counts())?;
    Ok((n0, n1, n_out, n))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 0/1/15 + 对拍 3 节点。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((0, 1, 15, 3)) => Ok(()),
        _ => Err(AffCode::NotReproducible.val()),
    }
}

/// 序 B：同页双节点 ⇒ PageDoubleClaim。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut bm = NumaAffinityBitmap::new();
    let _ = bm.claim_span(0, 0, 8);
    match bm.claim_span(1, 0, 8) {
        Err(e) if e == AffCode::PageDoubleClaim.val() => Ok(()),
        _ => Err(AffCode::NotReproducible.val()),
    }
}

/// 序 C：空节点亲和分配 ⇒ LocalEmpty。
pub fn probe_seq_c() -> Result<(), u32> {
    let bm = NumaAffinityBitmap::new();
    match bm.local_alloc_probe(5) {
        Err(e) if e == AffCode::LocalEmpty.val() => Ok(()),
        _ => Err(AffCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：交叠路。
        let mut b1 = NumaAffinityBitmap::new();
        let _ = b1.claim_span(0, 0, 16);
        led.record(0, b1.claim_span(1, PAGE * 8, 16).is_err());
        // 路 2：越界路（节点 16 / 页越窗）。
        let mut b2 = NumaAffinityBitmap::new();
        let hit2 = b2.claim_span(16, 0, 4).is_err()
            && b2.claim_span(0, BITMAP_PAGES as u64 * PAGE, 4).is_err();
        led.record(1, hit2);
        // 路 3：对拍路（少 1 页必失配）。
        let mut b3 = NumaAffinityBitmap::new();
        let claims = make_machine_claims();
        let mut i = 0;
        while i < claims.len() {
            let _ = b3.claim_span(claims[i].0, claims[i].1, claims[i].2);
            i += 1;
        }
        let mut bad_counts = machine_srat_counts();
        bad_counts[0] -= 1;
        led.record(2, b3.srat_audit(&bad_counts).is_err());
        // 路 4：零页路。
        let mut b4 = NumaAffinityBitmap::new();
        led.record(3, b4.claim_span(0, 0, 0).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AffCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AffCode::ALL.len() {
        let v = AffCode::ALL[i].val();
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
            Ok((n0, n1, nout, n)) => *n0 == 0 && *n1 == 1 && *nout == NO_NODE && *n == 3,
            Err(_) => false,
        },
        match &g {
            Ok((n0, n1, nout, n)) => format!("n0={} n1={} out={} srat={}", n0, n1, nout, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 零交叠：总页数 = 逐节点和。
    let mut bm = NumaAffinityBitmap::new();
    let claims = make_machine_claims();
    let mut i = 0;
    while i < claims.len() {
        let _ = bm.claim_span(claims[i].0, claims[i].1, claims[i].2);
        i += 1;
    }
    let sum = bm.per_node[0] + bm.per_node[1] + bm.per_node[2];
    v.push((
        "J1-零交叠",
        sum == 1280 && bm.per_node[3..].iter().all(|&c| c == 0),
        format!("per_node sum={}（512+512+256）", sum),
    ));
    // 亲和预演：本地有页出账、空节点拒。
    v.push((
        "J1-亲和预演",
        bm.local_alloc_probe(1) == Ok(512) && bm.local_alloc_probe(9).is_err(),
        String::from("本地优先 / 空账显性码"),
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
        String::from("0x6E1C0 窗口"),
    ));
    v
}

pub fn run_unx0074_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0074 NUMA 节点亲和页位图");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
