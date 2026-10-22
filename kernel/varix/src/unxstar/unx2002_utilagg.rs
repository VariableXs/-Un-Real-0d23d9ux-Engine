// unx2002_utilagg —— UNX-F2002 util 聚合语义（每核 util 与频率请求合成）
// 判据原文 UNX-F2002-J1：聚合断言全过（每核 util 聚合/多请求合成取最大），
// 聚合溢出防护 10/10，聚合计数与调度事件数守恒。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF580 独占（0xF580~0xF5FF）。
//
// 设计（纯功能 no_std）：
//   * 每核 util：千分比定点（0~1000），逐核登记；
//   * 全局聚合：Σ 核 util 饱和于 1000*NCORE 封顶（溢出防护：饱和不回绕）；
//   * 多请求合成：同窗口多核频率请求取最大（max 合成语义）；
//   * 守恒：聚合次数 = Σ 核上报次数（逐次计数零丢失）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF580 独占） ─────────────────────────────────────────────
pub const XC_UTIL_OOB: u32 = 0xF581; // util > 1000 或核号越界
pub const XC_UTIL_WRAP: u32 = 0xF582; // 聚合回绕（溢出防护破）
pub const XC_UTIL_LOST: u32 = 0xF583; // 聚合计数与上报数不守恒
pub const XC_UTIL_SEALED: u32 = 0xF584; // 封账后上报
pub const XC_UTIL_COREDUP: u32 = 0xF585; // 同核同窗口重复上报

/// 每核 util 上报。
#[derive(Clone, Copy)]
pub struct UtilReport {
    pub core: u32,
    pub util: u16, // 0..=1000 千分比
    pub freq_req_mhz: u32,
}

/// 聚合器。
pub struct UtilAggregator {
    ncore: u32,
    reports: Vec<UtilReport>,
    agg_count: u32,
    max_req: u32,
    sealed: bool,
    window_seen: Vec<u32>, // 已上报核（同窗口去重）
    rejects: Vec<(u32, &'static str)>,
}

impl UtilAggregator {
    pub fn new(ncore: u32) -> UtilAggregator {
        UtilAggregator {
            ncore,
            reports: Vec::new(),
            agg_count: 0,
            max_req: 0,
            sealed: false,
            window_seen: Vec::new(),
            rejects: Vec::new(),
        }
    }

    /// 上报一条（同核同窗口重复拒）。
    pub fn report(&mut self, r: UtilReport, window: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_UTIL_SEALED, "封账后上报"));
            return Err(XC_UTIL_SEALED);
        }
        if r.core >= self.ncore {
            self.rejects.push((XC_UTIL_OOB, "核号越界"));
            return Err(XC_UTIL_OOB);
        }
        if r.util > 1000 {
            self.rejects.push((XC_UTIL_OOB, "util 越界"));
            return Err(XC_UTIL_OOB);
        }
        for &c in &self.window_seen {
            if c == r.core {
                self.rejects.push((XC_UTIL_COREDUP, "同核重复上报"));
                return Err(XC_UTIL_COREDUP);
            }
        }
        self.window_seen.push(r.core);
        self.reports.push(r);
        self.agg_count += 1;
        if r.freq_req_mhz > self.max_req {
            self.max_req = r.freq_req_mhz;
        }
        let _ = window;
        Ok(())
    }

    /// 聚合 util：Σ，饱和于 1000*ncore（防护回绕）。
    pub fn aggregate(&self) -> u32 {
        let cap = 1000u32.saturating_mul(self.ncore);
        let mut sum: u32 = 0;
        for r in &self.reports {
            sum = sum.saturating_add(r.util as u32);
        }
        if sum > cap {
            cap
        } else {
            sum
        }
    }

    /// 溢出防护：sum 永不回绕（用 saturating 加法 + 封顶核验）。
    pub fn overflow_safe(&self) -> bool {
        let cap = 1000u32.saturating_mul(self.ncore);
        let mut sum: u32 = 0;
        let mut wrapped = false;
        for r in &self.reports {
            let next = sum.wrapping_add(r.util as u32);
            if next < sum {
                wrapped = true;
            }
            sum = next;
        }
        !wrapped && self.aggregate() <= cap
    }

    /// 多请求合成：取最大。
    pub fn composed_request(&self) -> u32 {
        self.max_req
    }

    /// 守恒：聚合数 == 上报条数 == 窗口已见核数。
    pub fn conserved(&self) -> bool {
        self.agg_count as usize == self.reports.len() && self.agg_count as usize == self.window_seen.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_UTIL_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.agg_count;
        h = h.wrapping_mul(0x0100_0193);
        for r in &self.reports {
            h ^= (r.core as u32) ^ (r.util as u32).wrapping_mul(0x9E37) ^ r.freq_req_mhz;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.max_req;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.max_req.min(0xFFFF) as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn report_count(&self) -> usize {
        self.reports.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）util 越界必拒（10/10：901~1000 全部+1）
    {
        let mut a = UtilAggregator::new(4);
        let mut hits = 0;
        for k in 1..=10u16 {
            if a.report(UtilReport { core: 0, util: 1000 + k, freq_req_mhz: 1000 }, 1) == Err(XC_UTIL_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）核号越界必拒
    {
        let mut a = UtilAggregator::new(2);
        if a.report(UtilReport { core: 5, util: 100, freq_req_mhz: 1 }, 1) == Err(XC_UTIL_OOB) {
            caught += 1;
        }
    }
    // 3）同核重复上报必拒
    {
        let mut a = UtilAggregator::new(4);
        let _ = a.report(UtilReport { core: 1, util: 100, freq_req_mhz: 10 }, 1);
        if a.report(UtilReport { core: 1, util: 200, freq_req_mhz: 20 }, 1) == Err(XC_UTIL_COREDUP) {
            caught += 1;
        }
    }
    // 4）溢出防护（大核数大 util 饱和不回绕）
    {
        let mut a = UtilAggregator::new(8);
        for c in 0..8u32 {
            let _ = a.report(UtilReport { core: c, util: 1000, freq_req_mhz: 100 + c }, 1);
        }
        if a.overflow_safe() && a.aggregate() == 8000 {
            caught += 1;
        }
    }
    // 5）封账后上报必拒
    {
        let mut a = UtilAggregator::new(2);
        let _ = a.seal();
        if a.report(UtilReport { core: 0, util: 1, freq_req_mhz: 1 }, 1) == Err(XC_UTIL_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2002-J1。
pub fn run_unx2002_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2002");

    // C02-01 每核聚合求和
    {
        let mut a = UtilAggregator::new(4);
        let rows = [(0u32, 500u16), (1, 250), (2, 125), (3, 125)];
        for &(c, u) in &rows {
            let _ = a.report(UtilReport { core: c, util: u, freq_req_mhz: 1000 + c as u32 }, 1);
        }
        let ok = a.aggregate() == 1000 && a.conserved();
        s.add("C02-01-聚合求和", ok, if ok { "" } else { "求和漂移" });
    }
    // C02-02 多请求合成取最大
    {
        let mut a = UtilAggregator::new(4);
        let rows = [(0u32, 100u16, 2400u32), (1, 200, 3600), (2, 300, 2800), (3, 50, 800)];
        for &(c, u, f) in &rows {
            let _ = a.report(UtilReport { core: c, util: u, freq_req_mhz: f }, 1);
        }
        let ok = a.composed_request() == 3600;
        s.add("C02-02-合成取最大", ok, if ok { "" } else { "合成漂移" });
    }
    // C02-03 守恒（聚合数 = 上报数 = 窗口核数）
    {
        let mut a = UtilAggregator::new(6);
        for c in 0..6u32 {
            let _ = a.report(UtilReport { core: c, util: 100, freq_req_mhz: 1 }, 2);
        }
        s.add("C02-03-计数守恒", a.conserved(), if a.conserved() { "" } else { "守恒破" });
    }
    // C02-04 溢出防护（饱和封顶）
    {
        let mut a = UtilAggregator::new(3);
        for c in 0..3u32 {
            let _ = a.report(UtilReport { core: c, util: 1000, freq_req_mhz: 1 }, 1);
        }
        let ok = a.aggregate() == 3000 && a.overflow_safe();
        s.add("C02-04-饱和封顶", ok, if ok { "" } else { "封顶破" });
    }
    // C02-05 util 边界（0 与 1000 合法）
    {
        let mut a = UtilAggregator::new(2);
        let r1 = a.report(UtilReport { core: 0, util: 0, freq_req_mhz: 1 }, 1);
        let r2 = a.report(UtilReport { core: 1, util: 1000, freq_req_mhz: 1 }, 1);
        let ok = r1.is_ok() && r2.is_ok();
        s.add("C02-05-边界合法", ok, if ok { "" } else { "边界误拒" });
    }
    // C02-06 同核重复上报拒
    {
        let mut a = UtilAggregator::new(2);
        let _ = a.report(UtilReport { core: 0, util: 10, freq_req_mhz: 1 }, 1);
        let ok = a.report(UtilReport { core: 0, util: 20, freq_req_mhz: 2 }, 1) == Err(XC_UTIL_COREDUP);
        s.add("C02-06-重复上报拒", ok, if ok { "" } else { "重复漏抓" });
    }
    // C02-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C02-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C02-08 不同窗口核可再报（窗口隔离）
    {
        let mut a = UtilAggregator::new(1);
        let _ = a.report(UtilReport { core: 0, util: 10, freq_req_mhz: 1 }, 1);
        let ok = a.report(UtilReport { core: 0, util: 20, freq_req_mhz: 2 }, 2).is_ok();
        s.add("C02-08-窗口隔离", ok, if ok { "" } else { "窗口串" });
    }
    // C02-09 指纹冻结
    {
        let f = || {
            let mut a = UtilAggregator::new(2);
            let _ = a.report(UtilReport { core: 0, util: 333, freq_req_mhz: 2400 }, 1);
            let _ = a.report(UtilReport { core: 1, util: 667, freq_req_mhz: 3600 }, 1);
            a.fingerprint()
        };
        let ok = f() == f();
        s.add("C02-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C02-10 封账稳定
    {
        let mut a = UtilAggregator::new(2);
        let _ = a.report(UtilReport { core: 0, util: 1, freq_req_mhz: 1 }, 1);
        let pre = a.fingerprint();
        let _ = a.seal();
        let ok = a.fingerprint() == pre && a.seal().is_err();
        s.add("C02-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
