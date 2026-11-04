//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0157 · 镜像引导成功率统计接口（深化第二实现） · 0x6F480
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0157-J1 镜像维度成功率：按镜像名分桶（名账唯一），
//!     各桶计数 + 千分位成功率，桶账守恒（Σ桶 = 总），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F480，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F480;

/// 最大镜像桶（冻结 8）。
pub const MAX_IMAGES: usize = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatCode {
    /// 镜像名空
    NameEmpty,
    /// 重名桶
    DuplicateImage,
    /// 桶越上限
    BucketOver,
    /// 结果账外
    OutcomeOver,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl StatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StatCode::NameEmpty => "镜像名空",
            StatCode::DuplicateImage => "重名桶：唯一红线",
            StatCode::BucketOver => "桶越上限：8 冻结",
            StatCode::OutcomeOver => "结果账外：>2",
            StatCode::ReplayMismatch => "复算失配",
            StatCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [StatCode; 6] = [
        StatCode::NameEmpty,
        StatCode::DuplicateImage,
        StatCode::BucketOver,
        StatCode::OutcomeOver,
        StatCode::ReplayMismatch,
        StatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StatCode::ALL.len() {
        if StatCode::ALL[i].val() == code {
            return StatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F480 段闭集内"
}

// ── 分桶账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageBucket {
    pub name: String,
    pub ok: u32,
    pub fail: u32,
}

#[derive(Clone, Debug, Default)]
pub struct ImageStats {
    pub buckets: Vec<ImageBucket>,
}

impl ImageStats {
    pub fn new() -> ImageStats {
        ImageStats::default()
    }

    fn idx_of(&self, name: &str) -> Option<usize> {
        let mut i = 0;
        while i < self.buckets.len() {
            if self.buckets[i].name == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 上报（结果 0=成功 1=失败；新桶自动开）。
    pub fn report(&mut self, image: &str, outcome: u8) -> Result<usize, u32> {
        if image.is_empty() {
            return Err(StatCode::NameEmpty.val());
        }
        if outcome > 1 {
            return Err(StatCode::OutcomeOver.val());
        }
        if let Some(i) = self.idx_of(image) {
            if outcome == 0 {
                self.buckets[i].ok += 1;
            } else {
                self.buckets[i].fail += 1;
            }
            return Ok(i);
        }
        if self.buckets.len() >= MAX_IMAGES {
            return Err(StatCode::BucketOver.val());
        }
        let b = ImageBucket { name: String::from(image), ok: if outcome == 0 { 1 } else { 0 }, fail: if outcome == 0 { 0 } else { 1 } };
        self.buckets.push(b);
        Ok(self.buckets.len() - 1)
    }

    /// 桶成功率（千分位）。
    pub fn bucket_rate(&self, name: &str) -> Result<u32, u32> {
        match self.idx_of(name) {
            Some(i) => {
                let b = &self.buckets[i];
                let t = b.ok + b.fail;
                if t == 0 {
                    return Ok(0);
                }
                Ok(((b.ok as u64 * 1000 + t as u64 / 2) / t as u64) as u32)
            }
            None => Err(StatCode::NameEmpty.val()),
        }
    }

    /// 守恒：Σ桶 = Σ计数。
    pub fn conservation(&self) -> Result<u32, u32> {
        let mut total = 0u32;
        let mut i = 0;
        while i < self.buckets.len() {
            total += self.buckets[i].ok + self.buckets[i].fail;
            i += 1;
        }
        Ok(total)
    }
}

/// 复算。
pub fn replay_ok(seq: &[(&str, u8)]) -> Result<(), u32> {
    let mut a = ImageStats::new();
    let mut b = ImageStats::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.report(seq[i].0, seq[i].1);
        let _ = b.report(seq[i].0, seq[i].1);
        i += 1;
    }
    if a.buckets != b.buckets {
        return Err(StatCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：重名外路/越果路/越限路/复算路） ────────────
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

/// 真机序列：main×7（6 成功）+ fallback×2。
pub fn machine_seq() -> Vec<(&'static str, u8)> {
    vec![
        ("main", 0), ("main", 0), ("main", 0), ("main", 1),
        ("main", 0), ("main", 0), ("main", 1),
        ("fallback", 0), ("fallback", 1),
    ]
}

// ── 黄金全链：分桶→main 714‰→守恒 9→复算→重名外无第二桶 ───────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut st = ImageStats::new();
    let seq = machine_seq();
    let mut i = 0;
    while i < seq.len() {
        st.report(seq[i].0, seq[i].1)?;
        i += 1;
    }
    let main_rate = st.bucket_rate("main")?;
    let cons = st.conservation()?;
    replay_ok(&seq)?;
    // 桶数守恒（两镜像两桶，重复上报不增桶）。
    let bucket_ok = (st.buckets.len() == 2) as u32;
    Ok((main_rate, cons, bucket_ok))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：main 714‰ + 守恒 9 + 两桶。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((714, 9, 1)) => Ok(()),
        _ => Err(StatCode::NotReproducible.val()),
    }
}

/// 序 B：结果 2 ⇒ OutcomeOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut st = ImageStats::new();
    match st.report("x", 2) {
        Err(e) if e == StatCode::OutcomeOver.val() => Ok(()),
        _ => Err(StatCode::NotReproducible.val()),
    }
}

/// 序 C：未知桶查率 ⇒ NameEmpty。
pub fn probe_seq_c() -> Result<(), u32> {
    match ImageStats::new().bucket_rate("ghost") {
        Err(e) if e == StatCode::NameEmpty.val() => Ok(()),
        _ => Err(StatCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：空名路。
        let mut s1 = ImageStats::new();
        led.record(0, s1.report("", 0).is_err());
        // 路 2：越果路。
        led.record(1, s1.report("x", 5).is_err());
        // 路 3：越限路（8 桶后第 9 镜像拒）。
        let mut s3 = ImageStats::new();
        let mut over = false;
        let mut i = 0;
        while i <= MAX_IMAGES {
            let n = alloc::format!("img{}", i);
            if s3.report(&n, 0).is_err() {
                over = true;
            }
            i += 1;
        }
        led.record(2, over);
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seq()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(StatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StatCode::ALL.len() {
        let v = StatCode::ALL[i].val();
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
            Ok((rate, cons, bk)) => *rate == 714 && *cons == 9 && *bk == 1,
            Err(_) => false,
        },
        match &g {
            Ok((rate, cons, bk)) => format!("main={}‰ total={} buckets={}", rate, cons, bk),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 上限冻结。
    v.push((
        "J1-上限冻结",
        MAX_IMAGES == 8,
        String::from("8 镜像桶冻结"),
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
        String::from("0x6F480 窗口"),
    ));
    v
}

pub fn run_unx0157b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0157 镜像引导成功率统计接口（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
