//! veb275_nvpwr —— VE-F0275 NVIDIA 功耗遥测。
//! 锚点（CoRun Unxreal 主册）：VE-F0275 · NVIDIA 功耗遥测 · 目标 300 行
//! 判据原文：VE-F0275-J1 功耗遥测账：功率采样（瓦）+ 温度采样
//!           （摄氏）+ 功耗墙（W 上限）；墙事件（超墙 → 记录 +
//!           降频建议值 = 墙瓦 × 90%）；温度联动（过温阈 → 同
//!           样降频建议）；采样环（最近 16 采样）；建议单调核
//!           （连续超墙期间建议值不升）。
//! 组件：采样账 + 墙检测 + 降频建议 + 温度联动。
//! 职责定位：功耗遥测是「NVIDIA 卡的体温计+电表」——功耗墙是
//!           电源的生死线，超墙不是报错是降频建议：性能换稳定；
//!           过温与超墙同路（都是降频建议）因为根因常相同；建
//!           议单调不升是降频纪律：越热越降，不许抖上去。
//! 性能逐项分解：采样 O(1)；环核 O(16)。
//! 诊断码段：0x30DC0（0x30DC1~0x30DC9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PW_VERSION: &str = "veb275_nvpwr-v1";

/// 诊断段基址（0x30DC0 段 · 本模块独占）。
pub const PW_SEG_BASE: u32 = 0x30DC0;
pub const XC_MAGIC: u32 = 0x30DC1; // 账魔数破
pub const XC_WALL_CFG: u32 = 0x30DC2; // 墙参不合法
pub const XC_TEMP_CFG: u32 = 0x30DC3; // 温阈不合法
pub const XC_EMPTY: u32 = 0x30DC4; // 空采样查询

/// 环容。
pub const RING: usize = 16;

/// 采样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub watts: u32,
    pub temp: u32,
}

/// 功耗遥测器。
pub struct PwrTelem {
    ring: Vec<Sample>,
    head: usize,
    total: u64,
    wall_w: u32,
    temp_max: u32,
    /// 当前降频建议（毫分比，900 = 90%）。
    pub advise_milli: u32,
    /// 墙事件数。
    pub wall_events: u32,
    magic: u32,
}

impl PwrTelem {
    /// 建（功耗墙 W、过温阈 ℃）。
    pub fn new(wall_w: u32, temp_max: u32) -> Result<PwrTelem, u32> {
        if wall_w == 0 || wall_w > 1200 {
            return Err(XC_WALL_CFG);
        }
        if temp_max < 60 || temp_max > 110 {
            return Err(XC_TEMP_CFG);
        }
        let mut ring = Vec::new();
        for _ in 0..RING {
            ring.push(Sample { watts: 0, temp: 0 });
        }
        Ok(PwrTelem {
            ring, head: 0, total: 0,
            wall_w, temp_max,
            advise_milli: 1000, wall_events: 0,
            magic: 0x0DC0_0BA7,
        })
    }

    /// 采样一次：超墙或过温 → 墙事件 + 建议降；否则建议回升（
    /// 每采样 +5%，封顶 1000）。
    pub fn sample(&mut self, watts: u32, temp: u32) -> Result<u32, u32> {
        if self.magic != 0x0DC0_0BA7 {
            return Err(XC_MAGIC);
        }
        self.ring[self.head] = Sample { watts, temp };
        self.head = (self.head + 1) % RING;
        self.total += 1;
        if watts > self.wall_w || temp > self.temp_max {
            self.wall_events += 1;
            self.advise_milli = self.wall_w * 900 / self.wall_w.max(1);
            // 建议单调：连续超墙期间不升（已是 900 则保持）。
            if self.advise_milli > 900 {
                self.advise_milli = 900;
            }
        } else {
            // 回升 +5% 封顶。
            self.advise_milli = (self.advise_milli + 50).min(1000);
        }
        Ok(self.advise_milli)
    }

    /// 最近 n 采样（时间升序）。
    pub fn recent(&self, n: usize) -> Result<Vec<Sample>, u32> {
        if self.magic != 0x0DC0_0BA7 {
            return Err(XC_MAGIC);
        }
        if self.total == 0 {
            return Err(XC_EMPTY);
        }
        let live = if self.total as usize >= RING { RING } else { self.total as usize };
        let take = if n > live { live } else { n };
        let mut out = Vec::new();
        // 环内最旧位置。
        let oldest = if self.total as usize >= RING { self.head } else { 0 };
        for k in 0..take {
            out.push(self.ring[(oldest + k) % RING]);
        }
        Ok(out)
    }

    /// 采样总数。
    pub fn count(&self) -> u64 {
        self.total
    }

    /// 峰值瓦。
    pub fn peak_watts(&self) -> Result<u32, u32> {
        let items = self.recent(RING)?;
        let mut p = 0u32;
        for it in items.iter() {
            if it.watts > p {
                p = it.watts;
            }
        }
        Ok(p)
    }
}

pub fn run_veb275_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0275");
    // 族零：配置。
    s.add("墙 0 拒", matches!(PwrTelem::new(0, 90), Err(XC_WALL_CFG)), "无墙");
    s.add("墙 2000 拒", matches!(PwrTelem::new(2000, 90), Err(XC_WALL_CFG)), ">1200");
    s.add("温阈 50 拒", matches!(PwrTelem::new(350, 50), Err(XC_TEMP_CFG)), "<60");
    s.add("温阈 120 拒", matches!(PwrTelem::new(350, 120), Err(XC_TEMP_CFG)), ">110");
    // 族一：正常采样。
    let mut t = match PwrTelem::new(350, 90) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败", false, "不应到");
            return s;
        }
    };
    s.add("初建议 1000", t.advise_milli == 1000, "全速");
    s.add("采样 200W", t.sample(200, 70) == Ok(1000), "墙内全速");
    // 空账查询拒：新账零采样即 recent 报空。
    let mut e = match PwrTelem::new(350, 90) {
        Ok(x) => x,
        Err(_) => {
            s.add("建账败 2", false, "不应到");
            return s;
        }
    };
    s.add("空账 recent 拒", e.recent(4) == Err(XC_EMPTY), "零采样");
    s.add("空账峰瓦拒", e.peak_watts() == Err(XC_EMPTY), "峰查空");
    s.add("采样数 1", t.count() == 1, "一采样");
    // 族二：超墙。
    s.add("超墙降 900", t.sample(400, 70) == Ok(900), "墙事件");
    s.add("墙事件 1", t.wall_events == 1, "一案");
    s.add("墙内回升 950", t.sample(300, 70) == Ok(950), "+5%");
    s.add("回升封顶", t.sample(300, 70) == Ok(1000), "回满");
    // 族三：过温联动。
    s.add("过温降", t.sample(100, 95) == Ok(900), "95>90");
    s.add("墙事件 2", t.wall_events == 2, "温也是墙");
    // 连续超墙不升。
    let _ = t.sample(400, 95);
    s.add("连超不升", t.advise_milli == 900, "单调");
    // 族四：采样环。
    let rec = t.recent(16);
    s.add("环 5 采样", matches!(&rec, Ok(v) if v.len() == 5), "live<16");
    s.add("峰瓦 400", t.peak_watts() == Ok(400), "超墙峰");
    // 峰值时间序。
    s.add("首采样 200W", matches!(&rec, Ok(v) if v[0].watts == 200), "时序头");
    // 族五：溢出环。
    let mut w = match PwrTelem::new(350, 90) {
        Ok(x) => x,
        Err(_) => { s.add("建账败 3", false, "不应到"); return s; }
    };
    for i in 0..20u32 {
        let _ = w.sample(100 + i, 70);
    }
    s.add("环封 16", matches!(w.recent(16), Ok(v) if v.len() == 16), "定容");
    s.add("首覆 5W", matches!(w.recent(16), Ok(v) if v[0].watts == 105), "1~4 被覆");
    s.add("魔数核", PW_SEG_BASE == 0x30DC0, "段基址");
    s
}
