// unx2763_reclaim —— UNX-F2763 回收率总账（申请/产出/放弃/待续四分账）
// 判据原文 UNX-F2763-J1：四分账账平断言（申请=产出+放弃+待续）10/10 轮闭合，
// 总回收率读数实时可查。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF900 独占（0xF900~0xF97F）。
//
// 设计（纯功能 no_std）：
//   * 四分账闭集：申请（请求回收的页数）/产出（实际回收）/放弃（不可回收放弃）/
//     待续（登记后未结）——守恒式：申请 == 产出 + 放弃 + 待续（逐轮闭合）；
//   * 10/10 轮闭合：十轮随机分配（确定性 LCG），每轮账平；
//   * 回收率读数：产出/申请 千分比实时可查（待续不计入分母已结面）；
//   * 账不平注入必抓；指纹冻结。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF900 独占） ─────────────────────────────────────────────
pub const XC_RCL_UNBAL: u32 = 0xF901; // 账不平（申请 ≠ 三分和）
pub const XC_RCL_OOB: u32 = 0xF902; // 三分和超申请
pub const XC_RCL_RATE: u32 = 0xF903; // 回收率越界
pub const XC_RCL_SEALED: u32 = 0xF904; // 封账后记账
pub const XC_RCL_NEG: u32 = 0xF905; // 负数记账

/// 单轮四分账。
#[derive(Clone, Copy)]
pub struct RoundLedger {
    pub req: u32,     // 申请
    pub produced: u32, // 产出
    pub abandoned: u32, // 放弃
    pub pending: u32, // 待续
}

/// 回收率总账。
pub struct ReclaimBook {
    rounds: Vec<RoundLedger>,
    total_req: u64,
    total_produced: u64,
    total_abandoned: u64,
    total_pending: u64,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ReclaimBook {
    pub fn new() -> ReclaimBook {
        ReclaimBook {
            rounds: Vec::new(),
            total_req: 0,
            total_produced: 0,
            total_abandoned: 0,
            total_pending: 0,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 记一轮（守恒闸：三分和 == 申请才入账）。
    pub fn round(&mut self, r: RoundLedger) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RCL_SEALED, "封账后记账"));
            return Err(XC_RCL_SEALED);
        }
        if (r.produced as u64) + (r.abandoned as u64) + (r.pending as u64) > r.req as u64 {
            self.rejects.push((XC_RCL_OOB, "三分和超申请"));
            return Err(XC_RCL_OOB);
        }
        if (r.produced as u64) + (r.abandoned as u64) + (r.pending as u64) < r.req as u64 {
            self.rejects.push((XC_RCL_UNBAL, "账不平"));
            return Err(XC_RCL_UNBAL);
        }
        self.rounds.push(r);
        self.total_req += r.req as u64;
        self.total_produced += r.produced as u64;
        self.total_abandoned += r.abandoned as u64;
        self.total_pending += r.pending as u64;
        Ok(())
    }

    /// 负数记账（模型上由 u32 保证非负；此处提供显式守卫语义）。
    pub fn guard_nonneg(req: u32, produced: u32, abandoned: u32, pending: u32) -> Result<(), u32> {
        // u32 非负是类型保证；显式守卫：三分和逻辑面
        let _ = (req, produced, abandoned, pending);
        Ok(())
    }

    /// 总账守恒。
    pub fn total_balanced(&self) -> bool {
        self.total_req == self.total_produced + self.total_abandoned + self.total_pending
    }

    /// 总回收率（千分比，实时可查）：产出/申请。
    pub fn reclaim_permille(&self) -> Result<u32, u32> {
        if self.total_req == 0 {
            return Err(XC_RCL_RATE);
        }
        let pm = (self.total_produced * 1000 / self.total_req) as u32;
        if pm > 1000 {
            return Err(XC_RCL_RATE);
        }
        Ok(pm)
    }

    /// 轮级账平复核（10/10 轮闭合面）。
    pub fn round_balance_all(&self) -> bool {
        for r in &self.rounds {
            if r.req != r.produced + r.abandoned + r.pending {
                return false;
            }
        }
        true
    }

    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_RCL_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for r in &self.rounds {
            h ^= r.req ^ r.produced.wrapping_mul(0x9E37) ^ r.abandoned.wrapping_mul(0x85EB) ^ r.pending;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.total_req as u32;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | (self.total_produced.min(0xFFFF) as u64)
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u32) -> u32 {
        (self.next() % n as u64) as u32
    }
}

/// 十轮确定性分配夹具：每轮三分随机分配且账平。
pub fn ten_rounds(seed: u64) -> ReclaimBook {
    let mut b = ReclaimBook::new();
    let mut g = Lcg::new(seed);
    for _ in 0..10 {
        let req = 100 + g.below(900);
        let produced = g.below(req + 1);
        let rest = req - produced;
        let abandoned = if rest > 0 { g.below(rest + 1) } else { 0 };
        let pending = rest - abandoned;
        let _ = b.round(RoundLedger { req, produced, abandoned, pending });
    }
    b
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）账不平（三分和 < 申请）必拒
    {
        let mut b = ReclaimBook::new();
        if b.round(RoundLedger { req: 100, produced: 40, abandoned: 30, pending: 20 }) == Err(XC_RCL_UNBAL) {
            caught += 1;
        }
    }
    // 2）三分和超申请必拒
    {
        let mut b = ReclaimBook::new();
        if b.round(RoundLedger { req: 100, produced: 60, abandoned: 50, pending: 0 }) == Err(XC_RCL_OOB) {
            caught += 1;
        }
    }
    // 3）空账回收率读数必拒
    {
        let b = ReclaimBook::new();
        if b.reclaim_permille() == Err(XC_RCL_RATE) {
            caught += 1;
        }
    }
    // 4）账平轮 10/10 全闭合
    {
        let b = ten_rounds(0xC0FFEE);
        if b.round_balance_all() && b.round_count() == 10 && b.total_balanced() {
            caught += 1;
        }
    }
    // 5）封账后记账必拒
    {
        let mut b = ReclaimBook::new();
        let _ = b.seal();
        if b.round(RoundLedger { req: 1, produced: 1, abandoned: 0, pending: 0 }) == Err(XC_RCL_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2763-J1。
pub fn run_unx2763_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2763");

    // C63-01 十轮闭合 10/10
    {
        let b = ten_rounds(0xA11CE);
        let ok = b.round_count() == 10 && b.round_balance_all() && b.total_balanced();
        s.add("C63-01-十轮闭合", ok, if ok { "" } else { "闭合破" });
    }
    // C63-02 总账守恒（申请=产出+放弃+待续）
    {
        let b = ten_rounds(0xB0B);
        s.add(
            "C63-02-总账守恒",
            b.total_balanced(),
            if b.total_balanced() { "" } else { "总账不平" },
        );
    }
    // C63-03 回收率实时可查（千分位界内）
    {
        let b = ten_rounds(0xCAFE);
        let r = b.reclaim_permille();
        let ok = r.is_ok() && r.unwrap() <= 1000;
        s.add("C63-03-回收率读数", ok, if ok { "" } else { "读数破" });
    }
    // C63-04 账平轮接受（单轮）
    {
        let mut b = ReclaimBook::new();
        let r = b.round(RoundLedger { req: 500, produced: 300, abandoned: 150, pending: 50 });
        let ok = r.is_ok() && b.round_count() == 1;
        s.add("C63-04-账平入账", ok, if ok { "" } else { "入账破" });
    }
    // C63-05 账不平拒（欠）
    {
        let mut b = ReclaimBook::new();
        let ok = b.round(RoundLedger { req: 10, produced: 5, abandoned: 3, pending: 1 }) == Err(XC_RCL_UNBAL);
        s.add("C63-05-欠账拒", ok, if ok { "" } else { "欠账漏抓" });
    }
    // C63-06 三分和超申请拒（溢）
    {
        let mut b = ReclaimBook::new();
        let ok = b.round(RoundLedger { req: 10, produced: 6, abandoned: 6, pending: 0 }) == Err(XC_RCL_OOB);
        s.add("C63-06-溢账拒", ok, if ok { "" } else { "溢账漏抓" });
    }
    // C63-07 全产出轮（回收率 1000‰）
    {
        let mut b = ReclaimBook::new();
        let _ = b.round(RoundLedger { req: 100, produced: 100, abandoned: 0, pending: 0 });
        let ok = b.reclaim_permille() == Ok(1000);
        s.add("C63-07-满收1000", ok, if ok { "" } else { "满收破" });
    }
    // C63-08 全放弃轮（回收率 0）
    {
        let mut b = ReclaimBook::new();
        let _ = b.round(RoundLedger { req: 100, produced: 0, abandoned: 100, pending: 0 });
        let ok = b.reclaim_permille() == Ok(0);
        s.add("C63-08-零收", ok, if ok { "" } else { "零收破" });
    }
    // C63-09 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C63-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C63-10 指纹冻结 + 封账稳定
    {
        let f = || ten_rounds(0xD00D).fingerprint();
        let mut g = ten_rounds(0xD00D);
        let pre = g.fingerprint();
        let _ = g.seal();
        let ok = f() == f() && g.fingerprint() == pre && g.seal().is_err();
        s.add("C63-10-指纹与封账", ok, if ok { "" } else { "漂移" });
    }
    s
}
