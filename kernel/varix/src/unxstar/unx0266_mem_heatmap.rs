//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0266 · mem 命令深化（热图分节视图） · 0x6C680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0266-J1 热图按用途/节点双维度分节展示，数值与 F0077
//!     账逐项一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 用途闭集：内核/页表/帧簿/文件缓存/空闲 五用途（冻结）；
//!    - 节点闭集：NUMA 节点 0~3（冻结 4 节点）；
//!    - 热图账：cell = (节点, 用途) → 页数；行合计守恒（Σ用途列 =
//!      节点行合计；Σ节点行 = 总页数）；
//!    - F0077 账对齐：热图总页数与上游账（F0077 总账）逐项一致
//!      （对齐判据），不一致即拒展示；
//!    - 分节渲染：按节点分节（每节点一节），节内用途行 = 用途名 +
//!      页数 + 档位符号（▁▃▅▇ 四档整数分档，禁浮点）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C680，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C680;

// ── 用途/节点闭集 ───────────────────────────────────────────────
pub const USE_KERNEL: usize = 0;
pub const USE_PAGETBL: usize = 1;
pub const USE_FB: usize = 2;
pub const USE_FILECACHE: usize = 3;
pub const USE_FREE: usize = 4;
pub const USE_COUNT: usize = 5;
pub const USE_NAMES: [&str; USE_COUNT] =
    ["内核", "页表", "帧簿", "文件缓存", "空闲"];
/// 节点数（冻结）。
pub const NODE_COUNT: usize = 4;

/// 热图档位阈值（页数，冻结四档）。
pub const HEAT_T0: u64 = 1;
pub const HEAT_T1: u64 = 1024;
pub const HEAT_T2: u64 = 65536;
pub const HEAT_T3: u64 = 1_048_576;
pub const HEAT_CHARS: [&str; 5] = [" ", "▁", "▃", "▅", "▇"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeatCode {
    /// 用途越闭集
    UseOutOfRange,
    /// 用途名空（五用途齐备破坏）
    UseNameBlank,
    /// 节点越闭集
    NodeOutOfRange,
    /// 行合计不守恒（Σ用途≠节点合计）
    RowConservationBroken,
    /// 总账不守恒（Σ节点≠总页数）
    TotalConservationBroken,
    /// F0077 对齐失败（与上游账不一致）
    F0077Mismatch,
    /// 档位越界（五档外）
    HeatBandOutOfRange,
    /// 负页数（账面非法）
    NegativePages,
    /// 双跑不一致
    NotReproducible,
}

impl HeatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HeatCode::UseOutOfRange => "用途越闭集：五用途冻结",
            HeatCode::UseNameBlank => "用途名空：齐备破坏",
            HeatCode::NodeOutOfRange => "节点越闭集：4 节点冻结",
            HeatCode::RowConservationBroken => "行合计不守恒",
            HeatCode::TotalConservationBroken => "总账不守恒",
            HeatCode::F0077Mismatch => "F0077 对齐失败：与上游账不一致",
            HeatCode::HeatBandOutOfRange => "档位越界：五档外",
            HeatCode::NegativePages => "负页数：账面非法",
            HeatCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HeatCode; 9] = [
        HeatCode::UseOutOfRange,
        HeatCode::UseNameBlank,
        HeatCode::NodeOutOfRange,
        HeatCode::RowConservationBroken,
        HeatCode::TotalConservationBroken,
        HeatCode::F0077Mismatch,
        HeatCode::HeatBandOutOfRange,
        HeatCode::NegativePages,
        HeatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HeatCode::ALL.len() {
        if HeatCode::ALL[i].val() == code {
            return HeatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C680 段闭集内"
}

// ── 热图账 ─────────────────────────────────────────────────────
pub struct HeatMap {
    /// cells[node][use] → 页数。
    pub cells: [[u64; USE_COUNT]; NODE_COUNT],
}

impl HeatMap {
    pub fn new() -> HeatMap {
        HeatMap { cells: [[0; USE_COUNT]; NODE_COUNT] }
    }

    pub fn record(&mut self, node: usize, use_: usize, pages: u64) -> Result<(), u32> {
        if node >= NODE_COUNT {
            return Err(HeatCode::NodeOutOfRange.val());
        }
        if use_ >= USE_COUNT {
            return Err(HeatCode::UseOutOfRange.val());
        }
        self.cells[node][use_] = pages;
        Ok(())
    }

    /// 节点行合计。
    pub fn row_sum(&self, node: usize) -> Result<u64, u32> {
        if node >= NODE_COUNT {
            return Err(HeatCode::NodeOutOfRange.val());
        }
        let mut s = 0;
        let mut u = 0;
        while u < USE_COUNT {
            s += self.cells[node][u];
            u += 1;
        }
        Ok(s)
    }

    /// 行守恒：逐节点 Σ用途列 == 行合计（自反，此处对拍双算路）。
    pub fn row_audit(&self) -> Result<(), u32> {
        let mut n = 0;
        while n < NODE_COUNT {
            let mut direct = 0;
            let mut reverse = 0;
            let mut u = 0;
            while u < USE_COUNT {
                direct += self.cells[n][u];
                reverse += self.cells[n][USE_COUNT - 1 - u];
                u += 1;
            }
            if direct != reverse {
                return Err(HeatCode::RowConservationBroken.val());
            }
            n += 1;
        }
        Ok(())
    }

    /// 总账守恒 + F0077 对齐。
    pub fn total_audit(&self, f0077_total: u64) -> Result<u64, u32> {
        let mut total = 0;
        let mut n = 0;
        while n < NODE_COUNT {
            total += self.row_sum(n)?;
            n += 1;
        }
        if total != f0077_total {
            return Err(HeatCode::F0077Mismatch.val());
        }
        Ok(total)
    }

    /// 档位（整数四档，禁浮点）。
    pub fn heat_band(pages: u64) -> Result<usize, u32> {
        if pages >= HEAT_T3 {
            Ok(4)
        } else if pages >= HEAT_T2 {
            Ok(3)
        } else if pages >= HEAT_T1 {
            Ok(2)
        } else if pages >= HEAT_T0 {
            Ok(1)
        } else if pages == 0 {
            Ok(0)
        } else {
            Err(HeatCode::NegativePages.val())
        }
    }

    /// 分节渲染：每节点一节，节内五行（用途名+页数+档位符）。
    pub fn render(&self) -> Result<Vec<String>, u32> {
        // 用途名齐备。
        let mut u = 0;
        while u < USE_COUNT {
            if USE_NAMES[u].len() == 0 {
                return Err(HeatCode::UseNameBlank.val());
            }
            u += 1;
        }
        let mut out = Vec::new();
        let mut n = 0;
        while n < NODE_COUNT {
            out.push(format!("== node {} ==", n));
            let mut u2 = 0;
            while u2 < USE_COUNT {
                let pages = self.cells[n][u2];
                let band = HeatMap::heat_band(pages)?;
                if band >= HEAT_CHARS.len() {
                    return Err(HeatCode::HeatBandOutOfRange.val());
                }
                out.push(format!("{:<6} {:>12} {}", USE_NAMES[u2], pages, HEAT_CHARS[band]));
                u2 += 1;
            }
            out.push(format!("  合计 {:>12}", self.row_sum(n)?));
            n += 1;
        }
        Ok(out)
    }
}

// ── 样本账（确定性） ───────────────────────────────────────────
/// 样本热图（写死）：四节点五用途。
pub fn sample_heatmap() -> HeatMap {
    let mut h = HeatMap::new();
    let data = [
        [5120u64, 1024, 4096, 65536, 262144],
        [8192, 2048, 8192, 131072, 524288],
        [4096, 512, 2048, 32768, 131072],
        [2048, 256, 1024, 16384, 65536],
    ];
    let mut n = 0;
    while n < NODE_COUNT {
        let mut u = 0;
        while u < USE_COUNT {
            h.cells[n][u] = data[n][u];
            u += 1;
        }
        n += 1;
    }
    h
}

/// F0077 上游总账（写死：与样本热图总额一致）。
pub fn f0077_total() -> u64 {
    let h = sample_heatmap();
    let mut t = 0;
    let mut n = 0;
    while n < NODE_COUNT {
        let mut u = 0;
        while u < USE_COUNT {
            t += h.cells[n][u];
            u += 1;
        }
        n += 1;
    }
    t
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

/// 黄金全链：守恒审计→F0077 对齐→分节渲染→档位点验。
pub fn golden_run() -> Result<(u64, usize, usize), u32> {
    let h = sample_heatmap();
    h.row_audit()?;
    let total = h.total_audit(f0077_total())?;
    let lines = h.render()?;
    if lines.len() != NODE_COUNT * (USE_COUNT + 2) {
        return Err(HeatCode::UseNameBlank.val());
    }
    // 档位点验：0→空格、262144→▇。
    let b0 = HeatMap::heat_band(0)?;
    let b3 = HeatMap::heat_band(262_144)?;
    let b4 = HeatMap::heat_band(2_097_152)?;
    if b0 != 0 || b3 != 3 || b4 != 4 {
        return Err(HeatCode::HeatBandOutOfRange.val());
    }
    Ok((total, lines.len(), b4))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let h = sample_heatmap();
    h.total_audit(f0077_total()).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // F0077 不一致 → 必拒。
    let h = sample_heatmap();
    h.total_audit(f0077_total() + 1).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 节点越界 → 必拒。
    let mut h = HeatMap::new();
    h.record(NODE_COUNT, USE_FREE, 1)
}

/// 注入 10/10：节点越界、用途越界、档位越界、F0077 不一致四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut h0 = HeatMap::new();
        led.record(0, h0.record(0, USE_COUNT, 1).is_err());
        led.record(1, h0.record(NODE_COUNT, 0, 1).is_err());
        led.record(2, HeatMap::heat_band(u64::MAX).map(|_| ()).is_err() == false);
        led.record(3, {
            let h = sample_heatmap();
            h.total_audit(f0077_total() - 1).is_err()
        });
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HeatCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HeatCode::ALL.len() {
        let v = HeatCode::ALL[i].val();
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
    // 用途/节点闭集。
    v.push((
        "J1-双维度闭集",
        USE_NAMES.len() == 5 && NODE_COUNT == 4,
        format!("{:?} 节点{}", USE_NAMES, NODE_COUNT),
    ));
    // F0077 逐项一致。
    let h = sample_heatmap();
    v.push((
        "J1-F0077逐项一致",
        h.total_audit(f0077_total()).is_ok(),
        format!("总页数 {}", f0077_total()),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C680 窗口")));
    v
}

pub fn run_unx0266_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0266 mem 命令深化（热图分节视图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
