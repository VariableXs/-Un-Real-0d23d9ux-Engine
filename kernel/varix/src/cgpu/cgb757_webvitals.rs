//! cgb757_webvitals —— CGPU-F0757 网页加载体验指标。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0757 网页加载体验指标  <SEG 0x4ce0>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0757；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌网页加载体验指标账，整数毫秒面）：
//!   1. 指标闭集五项（写死语义）：FCP 首内容绘制 / LCP 最大内容
//      绘制 / TTFB 首字节 / CLS 累积布局偏移（×1000 整数化）/
//      INP 交互延迟；全部整数毫秒/千分位，禁浮点；
//   2. 阈值表写死（Google CWV 口径整数化）：FCP good≤1800
//      poor>3000；LCP good≤2500 poor>4000；TTFB good≤800
//      poor>1800；CLS good≤100 poor>250（千分位）；INP good≤200
//      poor>500；
//   3. 样本账：每指标定容样本环（16 槽），样本数守恒：入账数 =
//      在环数 + 逐出数；三档（good/needs/poor）分布账随入账滚动；
//   4. 会话评分 = 五指标档位加权（写死权重表 good=0/poor=罚分），
//      评级三档（快/可/慢）取罚分阈值线；
//   5. 阶段瀑布账：导航→首字节→首绘→最大绘→可交互 五锚点单调
//      不回退（时间戳倒挂红码）；
//   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4ce0 段为本模块独占（SEG_BASE=0x4ce0），判别式
//   SEG_BASE+1..+N；与 0x4cc0/0x4d00 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const WV_VERSION: &str = "cgb757_webvitals-v1";

/// 诊断段基址（0x4ce0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4CE0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4ce1 指标号越界（0..=4 之外）。
pub const XC_BAD_METRIC: u16 = SEG_BASE + 1;
/// 0x4ce2 样本值越界（> 60000ms 或 CLS > 4000 千分位）。
pub const XC_BAD_SAMPLE: u16 = SEG_BASE + 2;
/// 0x4ce3 锚点倒挂（瀑布时间戳回退）。
pub const XC_BACKWARD_MARK: u16 = SEG_BASE + 3;
/// 0x4ce4 样本环账不守恒。
pub const XC_RING_UNBALANCED: u16 = SEG_BASE + 4;
/// 0x4ce5 空账评级（无样本不出会话分）。
pub const XC_RATE_EMPTY: u16 = SEG_BASE + 6 - 1; // 0x4ce5
/// 0x4ce6 锚点缺项（五锚未齐就评级）。
pub const XC_MARKS_INCOMPLETE: u16 = SEG_BASE + 6;
/// 0x4ce7 分布账漂移（三档和 ≠ 在环数）。
pub const XC_DIST_DRIFT: u16 = SEG_BASE + 7;
/// 0x4ce8 指标重复锚定。
pub const XC_MARK_DUP: u16 = SEG_BASE + 8;

/// 指标数（写死：5）。
pub const METRIC_CAP: usize = 5;
/// 样本环容量（写死：16）。
pub const RING_CAP: usize = 16;
/// 样本值上限（写死：60000ms）。
pub const SAMPLE_MAX: u32 = 60000;
/// CLS 上限（千分位，写死：4000 = 4.0）。
pub const CLS_MAX: u32 = 4000;

/// 指标号闭集：0=FCP 1=LCP 2=TTFB 3=CLS 4=INP。
pub const M_FCP: usize = 0;
pub const M_LCP: usize = 1;
pub const M_TTFB: usize = 2;
pub const M_CLS: usize = 3;
pub const M_INP: usize = 4;

/// 阈值三元组（good 上限 / poor 下限；之间 = needs）。
/// 写死：册内 §CWV 阈值表。CLS 列单位千分位。
pub const THRESH: [[u32; 2]; METRIC_CAP] = [
    [1800, 3000], // FCP
    [2500, 4000], // LCP
    [800, 1800],  // TTFB
    [100, 250],   // CLS（千分位）
    [200, 500],   // INP
];

/// 档位三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// 好。
    Good,
    /// 需改进。
    Needs,
    /// 差。
    Poor,
}

/// 档位判定。
pub fn band_of(metric: usize, val: u32) -> Result<Band, u16> {
    if metric >= METRIC_CAP {
        return Err(XC_BAD_METRIC);
    }
    let t = THRESH[metric];
    Ok(if val <= t[0] {
        Band::Good
    } else if val > t[1] {
        Band::Poor
    } else {
        Band::Needs
    })
}

/// 样本合法性（指标面越界即红）。
pub fn sample_legal(metric: usize, val: u32) -> Result<(), u16> {
    if metric >= METRIC_CAP {
        return Err(XC_BAD_METRIC);
    }
    let cap = if metric == M_CLS { CLS_MAX } else { SAMPLE_MAX };
    if val == 0 || val > cap {
        return Err(XC_BAD_SAMPLE);
    }
    Ok(())
}

/// 单指标样本环：16 槽 + 三档分布账。
pub struct SampleRing {
    buf: [Option<u32>; RING_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    /// 三档分布（good/needs/poor）。
    pub dist: [u32; 3],
}

impl SampleRing {
    /// 空环。
    pub fn new() -> Self {
        SampleRing {
            buf: [None; RING_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            dist: [0; 3],
        }
    }

    /// 入账（满则逐出最老；分布账滚动修正）。
    pub fn push(&mut self, metric: usize, val: u32) -> Result<Band, u16> {
        sample_legal(metric, val)?;
        let b = band_of(metric, val)?;
        let bi = match b {
            Band::Good => 0usize,
            Band::Needs => 1,
            Band::Poor => 2,
        };
        if self.in_ring < RING_CAP {
            self.in_ring += 1;
        } else {
            // 逐出最老样本并回退其档位。
            if let Some(old) = self.buf[self.head] {
                let ob = band_of(metric, old)?;
                let oi = match ob {
                    Band::Good => 0usize,
                    Band::Needs => 1,
                    Band::Poor => 2,
                };
                self.dist[oi] = self.dist[oi].saturating_sub(1);
            }
            self.evicted += 1;
        }
        self.buf[self.head] = Some(val);
        self.head = (self.head + 1) % RING_CAP;
        self.dist[bi] += 1;
        Ok(b)
    }

    /// 在环数。
    pub fn count(&self) -> usize {
        self.in_ring
    }

    /// 守恒：入账总数 = 在环 + 逐出；分布和 = 在环。
    pub fn conserved(&self, pushed: usize) -> Result<(), u16> {
        if self.in_ring + self.evicted != pushed {
            return Err(XC_RING_UNBALANCED);
        }
        if (self.dist[0] + self.dist[1] + self.dist[2]) as usize != self.in_ring {
            return Err(XC_DIST_DRIFT);
        }
        Ok(())
    }

    /// 中位样本（在环值升序取中）。
    pub fn median(&self) -> u32 {
        let mut v: Vec<u32> = self.buf.iter().flatten().copied().collect();
        if v.is_empty() {
            return 0;
        }
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        let n = v.len();
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2
        }
    }
}

/// 瀑布五锚点闭集：0 导航 1 首字节 2 首绘 3 最大绘 4 可交互。
pub const MARK_NAV: usize = 0;
pub const MARK_TTFB: usize = 1;
pub const MARK_FCP: usize = 2;
pub const MARK_LCP: usize = 3;
pub const MARK_READY: usize = 4;
pub const MARK_CAP: usize = 5;

/// 会话指标账总装：五环 + 瀑布锚 + 会话评分。
pub struct VitalsSession {
    rings: [SampleRing; METRIC_CAP],
    /// 锚点时间戳（ms；0 = 未锚）。
    marks: [u32; MARK_CAP],
    pushed: [usize; METRIC_CAP],
}

impl VitalsSession {
    /// 空会话。
    pub fn new() -> Self {
        VitalsSession {
            rings: [SampleRing::new(), SampleRing::new(), SampleRing::new(), SampleRing::new(), SampleRing::new()],
            marks: [0; MARK_CAP],
            pushed: [0; METRIC_CAP],
        }
    }

    /// 样本入账。
    pub fn sample(&mut self, metric: usize, val: u32) -> Result<Band, u16> {
        let b = self.rings[metric].push(metric, val)?;
        self.pushed[metric] += 1;
        Ok(b)
    }

    /// 锚点落账：时间戳必须 ≥ 前一锚（单调）；重复锚定红码。
    pub fn mark(&mut self, which: usize, ts: u32) -> Result<(), u16> {
        if which >= MARK_CAP {
            return Err(XC_BAD_METRIC);
        }
        if self.marks[which] != 0 {
            return Err(XC_MARK_DUP);
        }
        if which > 0 {
            let prev = self.marks[which - 1];
            if prev != 0 && ts < prev {
                return Err(XC_BACKWARD_MARK);
            }
        }
        self.marks[which] = ts;
        Ok(())
    }

    /// 五锚齐判定。
    pub fn marks_complete(&self) -> bool {
        self.marks.iter().all(|m| *m != 0)
    }

    /// 罚分权重表（写死：poor 罚分 FCP=2 LCP=3 TTFB=1 CLS=2 INP=3；
    /// needs 半罚）。
    pub fn penalty(&self) -> Result<u32, u16> {
        let w: [u32; METRIC_CAP] = [2, 3, 1, 2, 3];
        let mut p = 0u32;
        for m in 0..METRIC_CAP {
            if self.pushed[m] == 0 {
                return Err(XC_RATE_EMPTY);
            }
            let med = self.rings[m].median();
            match band_of(m, med)? {
                Band::Good => {}
                Band::Needs => p += w[m],
                Band::Poor => p += w[m] * 2,
            }
        }
        Ok(p)
    }

    /// 会话评级：罚分 ≤2 快 / ≤6 可 / >6 慢（写死线）。
    pub fn grade(&self) -> Result<u8, u16> {
        if !self.marks_complete() {
            return Err(XC_MARKS_INCOMPLETE);
        }
        let p = self.penalty()?;
        Ok(if p <= 2 {
            0 // 快
        } else if p <= 6 {
            1 // 可
        } else {
            2 // 慢
        })
    }

    /// 全账守恒核。
    pub fn conserved(&self) -> Result<(), u16> {
        for m in 0..METRIC_CAP {
            self.rings[m].conserved(self.pushed[m])?;
        }
        Ok(())
    }

    /// 指标环只读。
    pub fn ring(&self, metric: usize) -> Result<&SampleRing, u16> {
        self.rings.get(metric).ok_or(XC_BAD_METRIC)
    }
}

/// 自例程：五指标样本 → 档位分布 → 瀑布锚单调 → 评级三档 →
/// 守恒 → 错误路径（倒挂锚/重复锚/坏样本/越界指标/空账罚分）。
pub fn drill() -> Result<u32, u16> {
    let mut s = VitalsSession::new();
    // 主路径：全 good 样本（med 全落在 good 线内）。
    s.sample(M_FCP, 1200)?;
    s.sample(M_LCP, 2000)?;
    s.sample(M_TTFB, 500)?;
    s.sample(M_CLS, 50)?;
    s.sample(M_INP, 150)?;
    // 各指标三档直查。
    if band_of(M_FCP, 1800)? != Band::Good {
        return Err(XC_BAD_METRIC);
    }
    if band_of(M_FCP, 3000)? != Band::Needs {
        return Err(XC_BAD_METRIC);
    }
    if band_of(M_FCP, 3001)? != Band::Poor {
        return Err(XC_BAD_METRIC);
    }
    if band_of(M_CLS, 250)? != Band::Good {
        return Err(XC_BAD_METRIC);
    }
    if band_of(M_CLS, 251)? != Band::Needs {
        return Err(XC_BAD_METRIC);
    }
    // 瀑布锚：单调落账（导航锚 ts 用 1——0 是未锚哨兵）。
    s.mark(MARK_NAV, 1)?;
    s.mark(MARK_TTFB, 500)?;
    s.mark(MARK_FCP, 1200)?;
    s.mark(MARK_LCP, 2000)?;
    s.mark(MARK_READY, 2400)?;
    if !s.marks_complete() {
        return Err(XC_MARKS_INCOMPLETE);
    }
    // 评级：罚分 0 → 快(0)。
    if s.grade()? != 0 {
        return Err(XC_RATE_EMPTY);
    }
    s.conserved()?;

    // 档位漂移：TTFB 投 poor 样本 → 罚分 1×2=2 → 仍快线内。
    s.sample(M_TTFB, 2000)?;
    if s.penalty()? != 2 {
        return Err(XC_DIST_DRIFT);
    }
    // 追加 poor LCP → 环 [2000,5000] 中位 3500 → Needs +3 → 总 5 → 可(1)。
    s.sample(M_LCP, 5000)?;
    if s.grade()? != 1 {
        return Err(XC_RATE_EMPTY);
    }
    // 分布账核：TTFB 环 2 样本（1 good 1 poor）。
    let r = s.ring(M_TTFB)?;
    if r.count() != 2 || r.dist != [1, 0, 1] {
        return Err(XC_DIST_DRIFT);
    }

    // 环逐出守恒：FCP 连打 20 次（>16）→ 逐出 4。
    let mut s2 = VitalsSession::new();
    for i in 0..20u32 {
        s2.sample(M_FCP, 1000 + i)?;
    }
    s2.conserved()?;
    let r2 = s2.ring(M_FCP)?;
    if r2.count() != RING_CAP {
        return Err(XC_RING_UNBALANCED);
    }

    // 错误路径 1：锚点倒挂（TTFB 50 < NAV 100）。
    let mut s3 = VitalsSession::new();
    s3.mark(MARK_NAV, 100)?;
    if s3.mark(MARK_TTFB, 50) != Err(XC_BACKWARD_MARK) {
        return Err(XC_BACKWARD_MARK);
    }
    // 错误路径 2：重复锚定。
    if s3.mark(MARK_NAV, 200) != Err(XC_MARK_DUP) {
        return Err(XC_MARK_DUP);
    }
    // 错误路径 3：坏样本（0 值）。
    if s3.sample(M_FCP, 0) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 错误路径 4：CLS 越千分位上限。
    if s3.sample(M_CLS, CLS_MAX + 1) != Err(XC_BAD_SAMPLE) {
        return Err(XC_BAD_SAMPLE);
    }
    // 错误路径 5：指标号越界。
    if band_of(5, 1) != Err(XC_BAD_METRIC) {
        return Err(XC_BAD_METRIC);
    }
    // 错误路径 6：空账罚分。
    if s3.penalty() != Err(XC_RATE_EMPTY) {
        return Err(XC_RATE_EMPTY);
    }
    // 错误路径 7：锚未齐评级。
    if s3.grade() != Err(XC_MARKS_INCOMPLETE) {
        return Err(XC_MARKS_INCOMPLETE);
    }
    // 中位健全：环 20 样本中位 = 第 10/11 均值。
    // 样本 1000..1019 升序，环逐出前 4 → 在环 1004..1019，
    // 中位 = (1011+1012)/2 = 1011。
    if r2.median() != 1011 {
        return Err(XC_RING_UNBALANCED);
    }
    Ok(0)
}
