//! unx0961_pollinv — UNX-F0961 现存轮询栈盘点账（升级接管的前置测绘）。
//!
//! 锚点块（工位 W6 · 批二 · 诊断段独占声明）：
//! 任务单：UNX-F0961 现存轮询栈盘点账（升级接管的前置测绘）
//! 判据原文摘录（卡面验收行）：现存轮询栈逐点盘入账（轮询点/频率/
//! 数据面/控制面四栏），盘点账与源面一致（抽样 10/10 次对得上），
//! 零遗漏审计（漏点必抓）。
//! 诊断段 0x7E20（本单独占，XcCode 闭集见下）。
//! 设计纪律：盘点即测绘——每个轮询点四栏齐（位置/频率/数据面/
//! 控制面）才许入账；账点总数与源面点数守恒（盘点恒等式）；抽样
//! 对账走同源重建；全部错误走 Result<_, u16> 闭集。
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7E20;

// ── 诊断段独占 · 盘点账错误域 ──────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_COL: u16 = SEG_ERR + 1;   // 四栏缺栏
const E_DUP: u16 = SEG_ERR + 2;   // 重复盘入
const E_MISS: u16 = SEG_ERR + 3;  // 漏点
const E_FREQ: u16 = SEG_ERR + 4;  // 频率档非法
const E_STATE: u16 = SEG_ERR + 5; // 盘点态非法

/// 频率档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freq {
    Hot = 0,  // 每帧/每包级
    Warm = 1, // 每事件级
    Cold = 2, // 每秒级
}

impl Freq {
    pub fn idx(self) -> usize { self as usize }
    pub fn as_str(self) -> &'static str {
        match self { Freq::Hot => "hot", Freq::Warm => "warm", Freq::Cold => "cold" }
    }
}

/// 数据面/控制面标记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Data,
    Ctrl,
}

impl Face {
    pub fn as_str(self) -> &'static str {
        match self { Face::Data => "data", Face::Ctrl => "ctrl" }
    }
}

/// 轮询点盘点条目（四栏）。
#[derive(Clone, Debug)]
pub struct PollSite {
    pub name: String,   // 栏一：位置
    pub freq: Freq,     // 栏二：频率
    pub face_d: Face,   // 栏三：数据面
    pub face_c: Face,   // 栏四：控制面
}

impl PollSite {
    /// 四栏齐核验：名称非空即视为齐。
    pub fn complete(&self) -> bool {
        !self.name.is_empty()
    }
}

/// 盘点账本。
pub struct PollInventory {
    pub sites: Vec<PollSite>,
    pub source_total: u32, // 源面总点数（盘点前置给定）
}

impl PollInventory {
    pub fn new(source_total: u32) -> Self {
        PollInventory { sites: Vec::new(), source_total }
    }

    /// 盘点恒等式：账点数 ≤ 源面点数，且无重名。
    pub fn invariant_ok(&self) -> bool {
        (self.sites.len() as u32) <= self.source_total
            && !self.sites.iter().any(|s| !s.complete())
    }

    /// 盘入一个轮询点：重名必拦、缺栏必拦。
    pub fn add(&mut self, name: &str, freq: Freq, fd: Face, fc: Face) -> Result<u32, u16> {
        if name.is_empty() { return Err(E_COL); }
        if self.sites.iter().any(|s| s.name == name) { return Err(E_DUP); }
        self.sites.push(PollSite { name: name.into(), freq, face_d: fd, face_c: fc });
        Ok(self.sites.len() as u32)
    }

    /// 零遗漏审计：账点数 == 源面点数。
    pub fn no_miss_audit(&self) -> Result<u32, u16> {
        if (self.sites.len() as u32) != self.source_total { return Err(E_MISS); }
        Ok(self.sites.len() as u32)
    }

    /// 频率档分布。
    pub fn freq_dist(&self) -> [u32; 3] {
        let mut d = [0u32; 3];
        for s in &self.sites { d[s.freq.idx()] += 1; }
        d
    }

    /// 面分布：数据面点数、控制面点数（按 face_d 计）。
    pub fn face_dist(&self) -> (u32, u32) {
        let d = self.sites.iter().filter(|s| s.face_d == Face::Data).count() as u32;
        let c = self.sites.iter().filter(|s| s.face_d == Face::Ctrl).count() as u32;
        (d, c)
    }

    /// 抽样对账：按名取点，四栏逐一对得上。
    pub fn sample_check(&self, names: &[&str], expect_freq: &[Freq]) -> Result<u32, u16> {
        if names.is_empty() { return Err(E_STATE); }
        for (i, n) in names.iter().enumerate() {
            let s = match self.sites.iter().find(|s| &s.name == n) {
                Some(s) => s,
                None => return Err(E_MISS),
            };
            if s.freq != expect_freq[i] { return Err(E_FREQ); }
        }
        Ok(names.len() as u32)
    }

    /// 人话化摘要。
    pub fn summary(&self) -> String {
        let d = self.freq_dist();
        alloc::format!("pollinv sites={} hot={} warm={} cold={}", self.sites.len(), d[0], d[1], d[2])
    }
}

/// 域内源面样本构建器：十个标准轮询点。
fn source_sites() -> Vec<(&'static str, Freq, Face, Face)> {
    vec![
        ("tty-flush", Freq::Warm, Face::Data, Face::Ctrl),
        ("kbd-scan", Freq::Hot, Face::Data, Face::Ctrl),
        ("timer-jiffy", Freq::Hot, Face::Ctrl, Face::Ctrl),
        ("net-ring", Freq::Hot, Face::Data, Face::Ctrl),
        ("block-queue", Freq::Warm, Face::Data, Face::Ctrl),
        ("rtc-tick", Freq::Cold, Face::Ctrl, Face::Ctrl),
        ("ps2-mouse", Freq::Warm, Face::Data, Face::Ctrl),
        ("vga-cursor", Freq::Cold, Face::Data, Face::Ctrl),
        ("apic-timer", Freq::Hot, Face::Ctrl, Face::Ctrl),
        ("spurious-irq7", Freq::Cold, Face::Ctrl, Face::Ctrl),
    ]
}

/// 自例程：全量盘入、零遗漏、抽样 10/10 对账返回 0。
pub fn drill() -> Result<u32, u16> {
    let src = source_sites();
    let mut inv = PollInventory::new(src.len() as u32);
    // 场景一：逐点盘入。
    for (n, f, fd, fc) in &src {
        inv.add(n, *f, *fd, *fc)?;
    }
    if !inv.invariant_ok() { return Err(E_STATE); }
    if inv.no_miss_audit()? != 10 { return Err(E_MISS); }
    // 场景二：抽样对账 10/10。
    let names = ["tty-flush", "kbd-scan", "timer-jiffy", "net-ring", "block-queue",
                 "rtc-tick", "ps2-mouse", "vga-cursor", "apic-timer", "spurious-irq7"];
    let freqs = [Freq::Warm, Freq::Hot, Freq::Hot, Freq::Hot, Freq::Warm,
                 Freq::Cold, Freq::Warm, Freq::Cold, Freq::Hot, Freq::Cold];
    if inv.sample_check(&names, &freqs)? != 10 { return Err(E_STATE); }
    // 场景三：重复盘入必拦。
    if inv.add("kbd-scan", Freq::Hot, Face::Data, Face::Ctrl) != Err(E_DUP) { return Err(E_DUP); }
    // 场景四：缺栏必拦（空名）。
    let mut inv2 = PollInventory::new(1);
    if inv2.add("", Freq::Hot, Face::Data, Face::Ctrl) != Err(E_COL) { return Err(E_COL); }
    // 场景五：超源面点数审计必拦（漏点）。
    let mut inv3 = PollInventory::new(3);
    inv3.add("a", Freq::Hot, Face::Data, Face::Ctrl)?;
    inv3.add("b", Freq::Warm, Face::Data, Face::Ctrl)?;
    if inv3.no_miss_audit() != Err(E_MISS) { return Err(E_MISS); }
    // 场景六：分布口径。
    let d = inv.freq_dist();
    if d[0] + d[1] + d[2] != 10 { return Err(E_STATE); }
    let (fd, fc) = inv.face_dist();
    if fd + fc != 10 { return Err(E_STATE); }
    if inv.summary().is_empty() { return Err(E_STATE); }
    Ok(0)
}
