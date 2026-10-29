#![allow(dead_code)]
//! cgb758_persense —— CGPU-F0758 网页交互可感指标。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0758 网页交互可感指标  <SEG 0x6A00>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0758；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌网页交互可感指标账，整数毫秒面）：
//!   1. 交互类型闭集四项（写死语义）：TAP 点按 / KEY 按键 /
//!      SCROLL 滚动 / HOVER 悬停；感知延迟全部整数毫秒，禁浮点；
//!   2. 阈值表写死：TAP good≤100 poor>300；KEY good≤50 poor>200；
//!      SCROLL good≤50 poor>150；HOVER good≤100 poor>300；
//!   3. 样本账：每类型定容样本环（24 槽），样本数守恒：入账数 =
//!      在环数 + 逐出数；三档分布账随入账滚动修正；
//!   4. 帧预算账：8 槽定容，帧耗时 ≥34ms（≈2 帧 16.7ms 预算）记为
//!      卡顿；帧时间戳单调不回退（倒挂红码）；卡顿率按千分位整数化；
//!   5. 会话状态机：Active → Settled → Reported 单向推进，非法跃迁
//!      红码；评分 = 四类型中位档位加权罚分，评级三档（快/可/慢）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6A00 段为本模块独占（SEG_BASE=0x6A00），判别式
//!   SEG_BASE+1..+N；与 0x69e0/0x6A20 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const PS_VERSION: &str = "cgb758_persense-v1";

/// 诊断段基址（0x6A00 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6A00;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6A01 交互类型号越界（0..=3 之外）。
pub const XC_BAD_KIND: u16 = SEG_BASE + 1;
/// 0x6A02 感知延迟样本越界（0 或 > 10000ms）。
pub const XC_BAD_LATENCY: u16 = SEG_BASE + 2;
/// 0x6A03 样本环账不守恒。
pub const XC_RING_UNBALANCED: u16 = SEG_BASE + 3;
/// 0x6A04 三档分布账漂移。
pub const XC_DIST_DRIFT: u16 = SEG_BASE + 4;
/// 0x6A05 空账评分（无样本不出分）。
pub const XC_EMPTY_SCORE: u16 = SEG_BASE + 5;
/// 0x6A06 会话状态非法跃迁。
pub const XC_STATE_BAD: u16 = SEG_BASE + 6;
/// 0x6A07 帧时间戳倒挂。
pub const XC_FRAME_BACKWARD: u16 = SEG_BASE + 7;
/// 0x6A08 帧样本非法（0 或 > 1000ms）。
pub const XC_BAD_FRAME: u16 = SEG_BASE + 8;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 交互类型闭集：0=TAP 1=KEY 2=SCROLL 3=HOVER。
pub const K_TAP: usize = 0;
pub const K_KEY: usize = 1;
pub const K_SCROLL: usize = 2;
pub const K_HOVER: usize = 3;
/// 类型数（写死：4）。
pub const KIND_CAP: usize = 4;
/// 样本环容量（写死：24）。
pub const RING_CAP: usize = 24;
/// 延迟样本上限（写死：10000ms）。
pub const LAT_MAX: u32 = 10_000;
/// 帧账容量（写死：8）。
pub const FRAME_CAP: usize = 8;
/// 卡顿线（写死：34ms ≈ 2×16.7ms 帧预算）。
pub const JANK_LINE: u32 = 34;
/// 帧样本上限（写死：1000ms）。
pub const FRAME_MAX: u32 = 1000;

/// 阈值三元组（good 上限 / poor 下限；之间 = needs）。
pub const THRESH: [[u32; 2]; KIND_CAP] = [
    [100, 300], // TAP
    [50, 200],  // KEY
    [50, 150],  // SCROLL
    [100, 300], // HOVER
];

/// 罚分权重表（写死：TAP=2 KEY=3 SCROLL=1 HOVER=1）。
pub const WEIGHT: [u32; KIND_CAP] = [2, 3, 1, 1];

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

/// 档位 → 分布下标（0 good / 1 needs / 2 poor）。
fn band_idx(b: Band) -> usize {
    match b {
        Band::Good => 0,
        Band::Needs => 1,
        Band::Poor => 2,
    }
}

/// 档位判定。
pub fn band_of(kind: usize, lat: u32) -> Result<Band, u16> {
    if kind >= KIND_CAP {
        return Err(XC_BAD_KIND);
    }
    let t = THRESH[kind];
    Ok(if lat <= t[0] {
        Band::Good
    } else if lat > t[1] {
        Band::Poor
    } else {
        Band::Needs
    })
}

/// 样本合法性（延迟面越界即红）。
pub fn sample_legal(kind: usize, lat: u32) -> Result<(), u16> {
    if kind >= KIND_CAP {
        return Err(XC_BAD_KIND);
    }
    if lat == 0 || lat > LAT_MAX {
        return Err(XC_BAD_LATENCY);
    }
    Ok(())
}

/// 类型样本环：24 槽 + 三档分布账。
pub struct LatRing {
    buf: [Option<u32>; RING_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    /// 三档分布（good/needs/poor）。
    pub dist: [u32; 3],
}

impl LatRing {
    /// 空环。
    pub fn new() -> Self {
        LatRing {
            buf: [None; RING_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            dist: [0; 3],
        }
    }

    /// 入账（满则逐出最老；分布账滚动修正）。
    pub fn push(&mut self, kind: usize, lat: u32) -> Result<Band, u16> {
        sample_legal(kind, lat)?;
        let b = band_of(kind, lat)?;
        let bi = band_idx(b);
        if self.in_ring < RING_CAP {
            self.in_ring += 1;
        } else {
            // 逐出最老样本并回退其档位。
            if let Some(old) = self.buf[self.head] {
                let ob = band_of(kind, old)?;
                let oi = band_idx(ob);
                self.dist[oi] = self.dist[oi].saturating_sub(1);
            }
            self.evicted += 1;
        }
        self.buf[self.head] = Some(lat);
        self.head = (self.head + 1) % RING_CAP;
        self.dist[bi] += 1;
        Ok(b)
    }

    /// 在环数。
    pub fn count(&self) -> usize {
        self.in_ring
    }

    /// 升序副本（插入排序，确定性）。
    pub fn sorted(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.buf.iter().flatten().copied().collect();
        for i in 1..v.len() {
            let key = v[i];
            let mut j = i;
            while j > 0 && v[j - 1] > key {
                v[j] = v[j - 1];
                j -= 1;
            }
            v[j] = key;
        }
        v
    }

    /// 中位样本。
    pub fn median(&self) -> u32 {
        let v = self.sorted();
        let n = v.len();
        if n == 0 {
            return 0;
        }
        if n % 2 == 1 {
            v[n / 2]
        } else {
            (v[n / 2 - 1] + v[n / 2]) / 2
        }
    }

    /// p90 分位（升序取 (n*9)/10 槽）。
    pub fn p90(&self) -> u32 {
        let v = self.sorted();
        let n = v.len();
        if n == 0 {
            return 0;
        }
        let idx = (n * 9) / 10;
        v[if idx >= n { n - 1 } else { idx }]
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
}

/// 帧预算账：8 槽定容，卡顿计数 + 单调时间戳。
pub struct FrameLedger {
    times: [Option<u32>; FRAME_CAP],
    head: usize,
    in_ring: usize,
    evicted: usize,
    last_ts: u32,
    /// 卡顿帧数。
    pub jank: u32,
    /// 帧总数（含卡顿）。
    pub total: u32,
    /// 帧耗时总和（ms）。
    pub sum: u64,
}

impl FrameLedger {
    /// 空帧账。
    pub fn new() -> Self {
        FrameLedger {
            times: [None; FRAME_CAP],
            head: 0,
            in_ring: 0,
            evicted: 0,
            last_ts: 0,
            jank: 0,
            total: 0,
            sum: 0,
        }
    }

    /// 帧入账：ts 单调、dur 合法；≥34ms 记卡顿。
    pub fn push(&mut self, ts: u32, dur: u32) -> Result<(), u16> {
        if dur == 0 || dur > FRAME_MAX {
            return Err(XC_BAD_FRAME);
        }
        if self.total > 0 && ts < self.last_ts {
            return Err(XC_FRAME_BACKWARD);
        }
        if self.in_ring < FRAME_CAP {
            self.in_ring += 1;
        } else {
            self.evicted += 1;
        }
        self.times[self.head] = Some(dur);
        self.head = (self.head + 1) % FRAME_CAP;
        self.last_ts = ts;
        self.total += 1;
        self.sum += dur as u64;
        if dur >= JANK_LINE {
            self.jank += 1;
        }
        Ok(())
    }

    /// 卡顿率（千分位）：jank*1000/total；空账返回 0。
    pub fn jank_permil(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.jank * 1000) / self.total
    }

    /// 平均帧耗时（毫秒，向下取整）。
    pub fn avg_dur(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        (self.sum / self.total as u64) as u32
    }

    /// 守恒：帧总数 = 在环 + 逐出。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.total as usize != self.in_ring + self.evicted {
            return Err(XC_RING_UNBALANCED);
        }
        Ok(())
    }
}

/// 会话状态机：Active → Settled → Reported（单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// 采集中。
    Active,
    /// 已收口（可评分）。
    Settled,
    /// 已出报。
    Reported,
}

/// 交互可感会话总装：四类型环 + 帧账 + 状态机 + 评分。
pub struct SenseSession {
    rings: [LatRing; KIND_CAP],
    pushed: [usize; KIND_CAP],
    /// 帧预算账。
    pub frames: FrameLedger,
    state: SessionState,
    /// 会话标签（收口时命名）。
    pub label: String,
}

impl SenseSession {
    /// 空会话。
    pub fn new() -> Self {
        SenseSession {
            rings: [LatRing::new(), LatRing::new(), LatRing::new(), LatRing::new()],
            pushed: [0; KIND_CAP],
            frames: FrameLedger::new(),
            state: SessionState::Active,
            label: String::new(),
        }
    }

    /// 样本入账（仅 Active 态）。
    pub fn sample(&mut self, kind: usize, lat: u32) -> Result<Band, u16> {
        if self.state != SessionState::Active {
            return Err(XC_STATE_BAD);
        }
        let b = self.rings[kind].push(kind, lat)?;
        self.pushed[kind] += 1;
        Ok(b)
    }

    /// 帧入账（仅 Active 态）。
    pub fn frame(&mut self, ts: u32, dur: u32) -> Result<(), u16> {
        if self.state != SessionState::Active {
            return Err(XC_STATE_BAD);
        }
        self.frames.push(ts, dur)
    }

    /// 收口：Active → Settled；重复收口红码。
    pub fn settle(&mut self, label: &str) -> Result<(), u16> {
        if self.state != SessionState::Active {
            return Err(XC_STATE_BAD);
        }
        self.state = SessionState::Settled;
        self.label = label.to_string();
        Ok(())
    }

    /// 罚分（每类型中位档位 × 权重；缺类型即空账红码）。
    pub fn penalty(&self) -> Result<u32, u16> {
        if self.state != SessionState::Settled {
            return Err(XC_STATE_BAD);
        }
        let mut p = 0u32;
        for k in 0..KIND_CAP {
            if self.pushed[k] == 0 {
                return Err(XC_EMPTY_SCORE);
            }
            let med = self.rings[k].median();
            match band_of(k, med)? {
                Band::Good => {}
                Band::Needs => p += WEIGHT[k],
                Band::Poor => p += WEIGHT[k] * 2,
            }
        }
        Ok(p)
    }

    /// 评级：罚分 ≤1 快(0) / ≤4 可(1) / >4 慢(2)（写死线）。
    pub fn grade(&self) -> Result<u8, u16> {
        let p = self.penalty()?;
        Ok(if p <= 1 {
            0
        } else if p <= 4 {
            1
        } else {
            2
        })
    }

    /// 出报：Settled → Reported；报文 = 标签 + 四类型中位 + 卡顿率。
    pub fn report(&mut self) -> Result<String, u16> {
        if self.state != SessionState::Settled {
            return Err(XC_STATE_BAD);
        }
        let mut s = String::from("PS[");
        s.push_str(&self.label);
        s.push_str("] med=");
        for k in 0..KIND_CAP {
            s.push_str(&self.rings[k].median().to_string());
            s.push(',');
        }
        s.push_str(" jank=");
        s.push_str(&self.frames.jank_permil().to_string());
        self.state = SessionState::Reported;
        Ok(s)
    }

    /// 全账守恒核：四类型环守恒 + 帧账守恒。
    pub fn conserved(&self) -> Result<(), u16> {
        for k in 0..KIND_CAP {
            self.rings[k].conserved(self.pushed[k])?;
        }
        self.frames.conserved()
    }

    /// 类型环只读。
    pub fn ring(&self, kind: usize) -> Result<&LatRing, u16> {
        self.rings.get(kind).ok_or(XC_BAD_KIND)
    }
}

/// 自例程：四类型样本 → 档位分布 → 帧账卡顿 → 收口评分 →
/// 出报 → 守恒 → 错误路径（状态跃迁/倒挂帧/坏样本/越界类型/空账）。
pub fn drill() -> Result<u32, u16> {
    let mut s = SenseSession::new();
    // 主路径：全 good 样本。
    s.sample(K_TAP, 80)?;
    s.sample(K_KEY, 30)?;
    s.sample(K_SCROLL, 30)?;
    s.sample(K_HOVER, 60)?;
    // 档位边界直查。
    if band_of(K_TAP, 100)? != Band::Good {
        return Err(XC_BAD_KIND);
    }
    if band_of(K_TAP, 101)? != Band::Needs {
        return Err(XC_BAD_KIND);
    }
    if band_of(K_TAP, 301)? != Band::Poor {
        return Err(XC_BAD_KIND);
    }
    // 帧账：3 帧，其中 1 帧卡顿（40 ≥ 34）。
    s.frame(10, 16)?;
    s.frame(26, 16)?;
    s.frame(42, 40)?;
    if s.frames.jank != 1 {
        return Err(XC_BAD_FRAME);
    }
    if s.frames.jank_permil() != 333 {
        return Err(XC_RING_UNBALANCED);
    }
    if s.frames.avg_dur() != 24 {
        return Err(XC_BAD_FRAME);
    }
    // 收口 + 评分（全 good 中位 → 罚 0 → 快）。
    s.settle("case-a")?;
    if s.grade()? != 0 {
        return Err(XC_EMPTY_SCORE);
    }
    s.conserved()?;
    // 出报并核对标签。
    let rep = s.report()?;
    if !rep.contains("case-a") {
        return Err(XC_STATE_BAD);
    }
    // 错误路径 1：Reported 后再采样。
    if s.sample(K_TAP, 80) != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 错误路径 2：重复收口。
    if s.settle("x") != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 错误路径 3：坏帧（0 耗时）。
    let mut s2 = SenseSession::new();
    if s2.frame(10, 0) != Err(XC_BAD_FRAME) {
        return Err(XC_BAD_FRAME);
    }
    // 错误路径 4：帧时间戳倒挂。
    s2.frame(10, 16)?;
    if s2.frame(5, 16) != Err(XC_FRAME_BACKWARD) {
        return Err(XC_FRAME_BACKWARD);
    }
    // 错误路径 5：坏延迟样本（0 / 越上限）。
    if s2.sample(K_KEY, 0) != Err(XC_BAD_LATENCY) {
        return Err(XC_BAD_LATENCY);
    }
    if s2.sample(K_KEY, LAT_MAX + 1) != Err(XC_BAD_LATENCY) {
        return Err(XC_BAD_LATENCY);
    }
    // 错误路径 6：类型号越界。
    if band_of(4, 10) != Err(XC_BAD_KIND) {
        return Err(XC_BAD_KIND);
    }
    // 错误路径 7：未收口评分。
    if s2.penalty() != Err(XC_STATE_BAD) {
        return Err(XC_STATE_BAD);
    }
    // 错误路径 8：空账评分（已收口但无样本）。
    s2.settle("empty")?;
    if s2.penalty() != Err(XC_EMPTY_SCORE) {
        return Err(XC_EMPTY_SCORE);
    }
    // 环逐出与 p90：TAP 连打 30 次（>24）→ 逐出 6。
    let mut s3 = SenseSession::new();
    for i in 0..30u32 {
        s3.sample(K_TAP, 100 + i)?;
    }
    s3.conserved()?;
    let r3 = s3.ring(K_TAP)?;
    if r3.count() != RING_CAP {
        return Err(XC_RING_UNBALANCED);
    }
    // 在环 106..129，p90 槽 (24*9)/10=21 → 127。
    if r3.p90() != 127 {
        return Err(XC_RING_UNBALANCED);
    }
    // 分布账核：全 needs（101..300 之间）→ [0,24,0]。
    if r3.dist != [0, 24, 0] {
        return Err(XC_DIST_DRIFT);
    }
    Ok(0)
}
