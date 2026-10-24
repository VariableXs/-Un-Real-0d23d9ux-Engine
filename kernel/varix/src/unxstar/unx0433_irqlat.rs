//! unx0433_irqlat —— UNX-F0433 中断延迟度量（TSC 基准）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0433 · 中断延迟度量 · 目标 240 行
//! 判据原文：UNX-F0433-J1 延迟 = 帧内 TSC（异常框架盖戳）
//!           − 递交 TSC（设备侧盖戳）；度量账：样本 (递交戳,
//!           响应戳) 对逐对入账，延迟 = 差值（u64 环绕安全：
//!           用无符号减法自然处理回绕）；统计三量：min/max/
//!           均值（均值用除法截断，分母 0 拒绝）；离群立案：
//!           单样本 > 均值 × 离群倍数（默认 8×）立案但不剔
//!           除（账面保留）；时钟漂移核：同源两戳的差不得为
//!           负（负差 = 戳错序，立案）；样本容量上限 256，
//!           满后滑动丢弃最旧。
//! 组件：样本环（256 滑动）+ 环绕安全差值器 + 三量统计 +
//!       离群哨兵 + 负差立案 + 快照重放。
//! 职责定位：延迟是中断子系统的「体检血压」——没有基准数
//!   字，调度抖动、关中断过长、 storms 全靠猜；TSC 是唯一
//!   单调源，环绕安全差值是 TSC 度量的基本功；离群不剔除
//!   是「记账不许改历史」的纪律，立案留档让人看。
//! 性能逐项分解：入账 O(1)；三量 O(N)（巡检周期摊销）；
//!   离群 O(N)；滑动丢弃 O(1)。
//! 诊断码段：0x303C0（0x303C1~0x303C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IL_VERSION: &str = "unx0433_irqlat-v1";

/// 诊断段基址（0x303C0 段 · 本模块独占）。
pub const IL_SEG_BASE: u32 = 0x303C0;
pub const XC_NEG_DIFF: u32 = 0x303C1; // 负差（戳错序）
pub const XC_EMPTY: u32 = 0x303C2; // 空账统计
pub const XC_OUTLIER: u32 = 0x303C3; // 离群立案
pub const XC_IDEMPOTENT: u32 = 0x303C4; // 重放不一致
pub const XC_ZERO_DENOM: u32 = 0x303C5; // 均值分母零
pub const XC_MAGIC: u32 = 0x303C9; // 账魔数破

/// 样本容量（滑动环）。
pub const CAP: usize = 256;
/// 默认离群倍数。
pub const OUTLIER_X: u64 = 8;

/// 样本对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatSample {
    /// 递交 TSC（设备侧）。
    pub submit: u64,
    /// 响应 TSC（帧内）。
    pub respond: u64,
}

impl LatSample {
    /// 环绕安全延迟：无符号减法自然处理回绕。
    pub fn latency(&self) -> u64 {
        self.respond.wrapping_sub(self.submit)
    }

    /// 戳序核：环绕意义下 respond 不得早于 submit（差值超过
    /// 半环视为负差——TSC 实测中断延迟远小于半环）。
    pub fn ordered(&self) -> Result<(), u32> {
        let d = self.latency();
        if d > 0x8000_0000_0000_0000 {
            return Err(XC_NEG_DIFF);
        }
        Ok(())
    }
}

/// 三量统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LatStats {
    pub min: u64,
    pub max: u64,
    pub mean: u64,
    pub count: u64,
}

/// 延迟度量账。
pub struct LatBook {
    /// 样本环。
    pub ring: Vec<LatSample>,
    /// 离群立案数。
    pub outliers: u32,
}

impl LatBook {
    pub fn new() -> Self {
        LatBook { ring: Vec::new(), outliers: 0 }
    }

    /// 入账：戳序核 + 容量滑动。
    pub fn record(&mut self, submit: u64, respond: u64) -> Result<u64, u32> {
        let s = LatSample { submit, respond };
        s.ordered()?;
        let d = s.latency();
        if self.ring.len() >= CAP {
            self.ring.remove(0);
        }
        self.ring.push(s);
        Ok(d)
    }

    /// 三量统计（空账拒绝）。
    pub fn stats(&self) -> Result<LatStats, u32> {
        if self.ring.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut min = u64::MAX;
        let mut max = 0u64;
        let mut sum = 0u64;
        for s in self.ring.iter() {
            let d = s.latency();
            if d < min {
                min = d;
            }
            if d > max {
                max = d;
            }
            sum += d;
        }
        let n = self.ring.len() as u64;
        Ok(LatStats { min, max, mean: sum / n, count: n })
    }

    /// 离群巡检：> 均值×倍数 立案（不剔除）。
    pub fn scan_outliers(&mut self) -> Result<u32, u32> {
        let st = self.stats()?;
        if st.mean == 0 {
            return Err(XC_ZERO_DENOM);
        }
        let thresh = st.mean.saturating_mul(OUTLIER_X);
        let mut found = 0u32;
        for s in self.ring.iter() {
            if s.latency() > thresh {
                found += 1;
            }
        }
        self.outliers += found;
        Ok(found)
    }

    /// 满环核：容量到顶丢弃最旧。
    pub fn full_ring_keeps_newest(&mut self) -> bool {
        // 注入 CAP+5 个递增戳样本，环首应为第 6 个。
        let mut ok = true;
        for i in 0..(CAP as u64 + 5) {
            if self.record(i * 10, i * 10 + 3).is_err() {
                ok = false;
                break;
            }
        }
        ok && self.ring.len() == CAP && self.ring[0].submit == 5 * 10
    }

    /// 重放核：同事件序同统计。
    pub fn replay_ok(&self) -> Result<(), u32> {
        let mut fresh = LatBook::new();
        for s in self.ring.iter() {
            let _ = fresh.record(s.submit, s.respond);
        }
        if fresh.ring != self.ring {
            return Err(XC_IDEMPOTENT);
        }
        Ok(())
    }
}

/// 判据入口：UNX-F0433。
pub fn run_unx0433_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0433_irqlat");
    // 族一：差值与序。
    let s1 = LatSample { submit: 1000, respond: 1042 };
    cs.add("延迟 42", s1.latency() == 42, "差值");
    cs.add("序过", s1.ordered().is_ok(), "单调");
    let s2 = LatSample { submit: 1042, respond: 1000 };
    cs.add("逆序立案", s2.ordered() == Err(XC_NEG_DIFF), "负差");
    // 族二：环绕安全。
    let s3 = LatSample { submit: u64::MAX - 10, respond: 5 };
    cs.add("回绕延迟 16", s3.latency() == 16, "跨零点");
    cs.add("回绕序过", s3.ordered().is_ok(), "半环内");
    let s4 = LatSample { submit: 0, respond: 0xFFFF_FFFF_FFFF_0000 };
    cs.add("大负差拒", s4.ordered() == Err(XC_NEG_DIFF), "超半环");
    // 族三：入账与统计。
    let mut b = LatBook::new();
    let _ = b.record(100, 108);
    let _ = b.record(200, 204);
    let _ = b.record(300, 320);
    let st = b.stats();
    cs.add("三样本入账", st.as_ref().map(|x| x.count).unwrap_or(0) == 3, "容量");
    cs.add("min=4", st.as_ref().map(|x| x.min).unwrap_or(0) == 4, "最短");
    cs.add("max=20", st.as_ref().map(|x| x.max).unwrap_or(0) == 20, "最长");
    cs.add("mean=10", st.as_ref().map(|x| x.mean).unwrap_or(0) == 10, "32/3 截断");
    // 族四：空账。
    cs.add("空账拒统计", LatBook::new().stats() == Err(XC_EMPTY), "无样本");
    // 族五：滑动环。
    let mut b2 = LatBook::new();
    cs.add("满环保新", b2.full_ring_keeps_newest(), "CAP 顶丢首");
    cs.add("环容量 256", b2.ring.len() == CAP, "滑窗");
    // 族六：离群。
    let mut b3 = LatBook::new();
    for i in 0..8 {
        let _ = b3.record(i * 100, i * 100 + 10); // 均值 10
    }
    let _ = b3.record(800, 1200); // 400：离群
    let found = b3.scan_outliers();
    cs.add("离群检出 1", found == Ok(1), "400 > 80 阈");
    cs.add("立案计数", b3.outliers == 1, "账面");
    cs.add("离群不剔除", b3.ring.len() == 9, "账面保留");
    // 族七：全零均值分母。
    let mut b4 = LatBook::new();
    let _ = b4.record(100, 100);
    let _ = b4.record(200, 200);
    cs.add("零均值分母拒", b4.scan_outliers() == Err(XC_ZERO_DENOM), "0×8 无义");
    // 族八：重放。
    let mut b5 = LatBook::new();
    let _ = b5.record(1, 3);
    let _ = b5.record(2, 9);
    cs.add("重放一致", b5.replay_ok().is_ok(), "同序同环");
    // 族九：快照等价（记录即快照）。
    let mut b6 = LatBook::new();
    let _ = b6.record(10, 12);
    let snap = b6.ring.clone();
    let mut b7 = LatBook::new();
    let _ = b7.record(10, 12);
    cs.add("快照等价", b7.ring == snap, "独立账同环");
    // 族十：大延迟不假阴。
    let mut b8 = LatBook::new();
    for i in 0..16 {
        let _ = b8.record(i * 50, i * 50 + 5); // 均值 5
    }
    let _ = b8.record(800, 800 + 48); // 48 < 40? 否 48 > 40 → 离群
    cs.add("近阈检出", b8.scan_outliers() == Ok(1), "48 > 40");
    cs
}
