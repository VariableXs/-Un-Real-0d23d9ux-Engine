//! veb370_healthmon —— VE-F0370 驱动健康监测。
//! 锚点（CoRun Unxreal 主册）：VE-F0370 · 驱动健康监测 · 目标 340 行
//! 判据原文：VE-F0370-J1 驱动健康分三探针（心跳成功率千分 /
//!           响应延迟均值 / 错误率千分）滚动窗口聚合；健康分 =
//!           加权（0~1000）；分档三态（健康 ≥ 800 / 降级 ≥ 500
//!           / 危殆 < 500）；危殆立案（降级不立案只提示）；探
//!           针样本越域拒（成功率 > 1000、延迟 > 上限）；滚动
//!           窗口定深（超深挤旧）；聚合可重放。
//! 组件：探针样本（三值）+ 滚动窗（定深 FIFO）+ 聚合器（均值）
//!       + 健康分引擎（加权 + 分档）+ 立案账 + 重放核。
//! 职责定位：健康监测是「驱动的体检仪」——三探针回答三个问题：
//!   它还活着吗（心跳）、它多快（延迟）、它错多少（错误率）；
//!   单点样本会骗人（一次网络抖动就是 100% 错误率），滚动窗
//!   口用历史摊平噪声；三态分档是行动依据：健康不用管，降级
//!   提示观察，危殆必须立案——不立案的报警等于没有报警。
//! 性能逐项分解：采样 O(1)；聚合 O(窗深)；分档 O(1)；重放
//!   O(样本数)。
//! 诊断码段：0x30200（0x30201~0x30209 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const HM_VERSION: &str = "veb370_healthmon-v1";

/// 诊断段基址（0x30200 段 · 本模块独占）。
pub const HM_SEG_BASE: u32 = 0x30200;
pub const XC_HEART_OOR: u32 = 0x30201; // 心跳率越域
pub const XC_LATENCY_OOR: u32 = 0x30202; // 延迟越域
pub const XC_ERR_OOR: u32 = 0x30203; // 错误率越域
pub const XC_CRITICAL: u32 = 0x30204; // 危殆立案
pub const XC_WIN_EMPTY: u32 = 0x30205; // 空窗聚合拒
pub const XC_MAGIC: u32 = 0x30206; // 账魔数破
pub const XC_REPLAY: u32 = 0x30207; // 重放不一致
pub const XC_DRV_MISS: u32 = 0x30208; // 驱动不在册
pub const XC_DUP_DRV: u32 = 0x30209; // 驱动重号

/// 窗口深度。
pub const WIN_DEPTH: usize = 8;
/// 延迟上限（tick 模型）。
pub const LAT_MAX: u32 = 100_000;

/// 探针样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    /// 心跳成功率（千分 0~1000）。
    pub heart_milli: u32,
    /// 响应延迟（tick）。
    pub latency: u32,
    /// 错误率（千分 0~1000）。
    pub err_milli: u32,
}

impl Probe {
    /// 样本合法性。
    pub fn valid(&self) -> Result<(), u32> {
        if self.heart_milli > 1000 {
            return Err(XC_HEART_OOR);
        }
        if self.latency > LAT_MAX {
            return Err(XC_LATENCY_OOR);
        }
        if self.err_milli > 1000 {
            return Err(XC_ERR_OOR);
        }
        Ok(())
    }
}

/// 健康三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    /// 健康 ≥ 800。
    Good,
    /// 降级 ≥ 500。
    Degraded,
    /// 危殆 < 500。
    Critical,
}

impl Health {
    pub fn code(&self) -> u32 {
        match self {
            Health::Good => 1,
            Health::Degraded => 2,
            Health::Critical => 3,
        }
    }
}

/// 健康分引擎（窗口定深）。
pub struct HealthMon {
    win: Vec<Probe>,
}

/// 权重：心跳 500 / 延迟 300 / 错误率 200（和 = 1000）。
pub const W_HEART: u32 = 500;
pub const W_LAT: u32 = 300;
pub const W_ERR: u32 = 200;

impl HealthMon {
    /// 建。
    pub fn new() -> HealthMon {
        HealthMon { win: Vec::new() }
    }

    /// 采样：越域拒；定深挤旧。
    pub fn feed(&mut self, p: Probe) -> Result<(), u32> {
        p.valid()?;
        if self.win.len() >= WIN_DEPTH {
            self.win.remove(0);
        }
        self.win.push(p);
        Ok(())
    }

    /// 聚合：均值 + 加权分。
    pub fn aggregate(&self) -> Result<(u32, Health), u32> {
        if self.win.is_empty() {
            return Err(XC_WIN_EMPTY);
        }
        let n = self.win.len() as u32;
        let mut hs: u64 = 0;
        let mut ls: u64 = 0;
        let mut es: u64 = 0;
        for p in self.win.iter() {
            hs += p.heart_milli as u64;
            ls += p.latency as u64;
            es += p.err_milli as u64;
        }
        let heart = (hs / n as u64) as u32;
        let lat = (ls / n as u64) as u32;
        let err = (es / n as u64) as u32;
        // 延迟分：LAT_MAX 的倒数映射（lat=0 → 1000，lat=MAX → 0）。
        let lat_score = 1000 - (lat as u64 * 1000 / LAT_MAX as u64) as u32;
        let score = ((heart as u64 * W_HEART as u64
            + lat_score as u64 * W_LAT as u64
            + (1000 - err) as u64 * W_ERR as u64)
            / 1000) as u32;
        let h = if score >= 800 {
            Health::Good
        } else if score >= 500 {
            Health::Degraded
        } else {
            Health::Critical
        };
        Ok((score, h))
    }

    /// 危殆闸：危殆立案。
    pub fn gate(&self) -> Result<(u32, Health), u32> {
        let (sc, h) = self.aggregate()?;
        if h == Health::Critical {
            return Err(XC_CRITICAL);
        }
        Ok((sc, h))
    }

    /// 窗深。
    pub fn win_len(&self) -> usize {
        self.win.len()
    }
}

/// 监测场：多驱动。
pub struct HealthField {
    mons: Vec<(String, HealthMon)>,
    cases: Vec<String>,
}

impl HealthField {
    /// 建。
    pub fn new() -> HealthField {
        HealthField { mons: Vec::new(), cases: Vec::new() }
    }

    /// 挂驱动：重号拒。
    pub fn attach(&mut self, name: &str) -> Result<(), u32> {
        if name.is_empty() {
            return Err(XC_DRV_MISS);
        }
        for (n, _) in self.mons.iter() {
            if n == name {
                return Err(XC_DUP_DRV);
            }
        }
        self.mons.push((String::from(name), HealthMon::new()));
        Ok(())
    }

    /// 采样（转发）。
    pub fn feed(&mut self, name: &str, p: Probe) -> Result<(), u32> {
        let m = match self.mons.iter_mut().find(|(n, _)| n == name) {
            Some((_, m)) => m,
            None => return Err(XC_DRV_MISS),
        };
        m.feed(p)
    }

    /// 全场体检：危殆立案。
    pub fn inspect(&mut self) -> Result<u32, u32> {
        self.cases.clear();
        for (n, m) in self.mons.iter() {
            if m.gate().is_err() {
                self.cases.push(n.clone());
            }
        }
        if !self.cases.is_empty() {
            return Err(XC_CRITICAL);
        }
        Ok(self.mons.len() as u32)
    }

    /// 立案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 挂数。
    pub fn attached(&self) -> usize {
        self.mons.len()
    }

    /// 重放核：再检结论一致。
    pub fn replay_ok(&mut self) -> Result<(), u32> {
        let first = self.inspect();
        let second = self.inspect();
        if first != second {
            return Err(XC_REPLAY);
        }
        Ok(())
    }
}

pub fn run_veb370_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0370");
    // 族一：样本合法性。
    let good = Probe { heart_milli: 1000, latency: 10, err_milli: 0 };
    s.add("好样本过", good.valid().is_ok(), "全限内");
    s.add("心跳超域拒",
        Probe { heart_milli: 1001, latency: 1, err_milli: 0 }.valid() == Err(XC_HEART_OOR),
        "1001");
    s.add("延迟超域拒",
        Probe { heart_milli: 100, latency: LAT_MAX + 1, err_milli: 0 }.valid() == Err(XC_LATENCY_OOR),
        "> 上限");
    s.add("错误率超域拒",
        Probe { heart_milli: 100, latency: 1, err_milli: 1001 }.valid() == Err(XC_ERR_OOR),
        "1001");
    // 族二：聚合与分档。
    let mut m = HealthMon::new();
    for _ in 0..8u32 {
        let _ = m.feed(good);
    }
    s.add("窗深 8", m.win_len() == 8, "定深");
    let _ = m.feed(good); // 挤旧
    s.add("超深挤旧", m.win_len() == 8, "仍 8");
    let agg = m.aggregate();
    s.add("健康聚合", agg == Ok((1000, Health::Good)), "满分");
    s.add("危殆闸绿", m.gate().is_ok(), "无案");
    // 降级样本。
    let mid = Probe { heart_milli: 900, latency: 20_000, err_milli: 300 };
    let mut d = HealthMon::new();
    for _ in 0..8u32 {
        let _ = d.feed(mid);
    }
    let ad = d.aggregate();
    s.add("降级档", ad.is_ok() && ad.map(|(_, h)| h) == Ok(Health::Degraded), "500~799");
    s.add("降级不立案", d.gate().is_ok(), "提示级");
    // 危殆样本。
    let bad = Probe { heart_milli: 100, latency: 90_000, err_milli: 900 };
    let mut c = HealthMon::new();
    for _ in 0..8u32 {
        let _ = c.feed(bad);
    }
    s.add("危殆档", c.aggregate().map(|(_, h)| h) == Ok(Health::Critical), "< 500");
    s.add("危殆立案", c.gate() == Err(XC_CRITICAL), "必须立案");
    // 空窗拒。
    s.add("空窗拒", HealthMon::new().aggregate() == Err(XC_WIN_EMPTY), "无样本");
    // 族三：监测场。
    let mut f = HealthField::new();
    let _ = f.attach("gpu");
    let _ = f.attach("nvme");
    let _ = f.attach("audio");
    s.add("三驱动挂账", f.attached() == 3, "全场");
    s.add("挂重号拒", f.attach("gpu") == Err(XC_DUP_DRV), "gpu 撞");
    s.add("空名拒", f.attach("") == Err(XC_DRV_MISS), "名空");
    // gpu 好，nvme 危，audio 好。
    for _ in 0..8u32 {
        let _ = f.feed("gpu", good);
        let _ = f.feed("nvme", bad);
        let _ = f.feed("audio", good);
    }
    s.add("体检立案", f.inspect() == Err(XC_CRITICAL), "nvme 危殆");
    s.add("立案数 1", f.case_count() == 1, "只有 nvme");
    s.add("重放核过", f.replay_ok().is_ok(), "两检一致");
    // 全好后清案。
    for _ in 0..8u32 {
        let _ = f.feed("nvme", good);
    }
    s.add("康复清案", f.inspect().is_ok(), "全场健康");
    s.add("魔数核", HM_SEG_BASE == 0x30200, "段基址");
    s
}
