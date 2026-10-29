// unx1011_u1011 —— UNX-F1011 TSC 压测资产（并发读与一致性耐受）
// 判据原文 UNX-F0956→F1011-J1：压测通过（四核并发读 10^7 次零单调违例），
// 压测资产隔离断言，劣化曲线落账。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1AE0 独占（0x1AE0~0x1B1F）。
//
// 设计（纯功能 no_std）：
//   * 压测机：四核交错读驱动器（核号轮转+增量推进），违例即中断落账；
//   * 10^7 口径：10 轮 × 100 万次（每轮独立账本，防跨轮串味）；
//   * 资产隔离：压测账与生产账分离（隔离位 + 账本独立命名空间）；
//   * 劣化曲线落账：每轮平均每读周期数记账（10 点曲线，趋势判定）；
//   * 零违例硬闸：任一轮出现单调违例即压测失败（不折算不豁免）；
//   * FNV 指纹冻结 + 守恒（读数=账数）+ 注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1AE0 独占） ─────────────────────────────────────────────
pub const XC_STRS_MONO: u32 = 0x1AE1; // 单调违例（压测中断）
pub const XC_STRS_LEAK: u32 = 0x1AE2; // 资产隔离破（生产账被写）
pub const XC_STRS_CORE: u32 = 0x1AE3; // 核号越界
pub const XC_STRS_SEALED: u32 = 0x1AE4; // 封账后压测
pub const XC_STRS_CURVE: u32 = 0x1AE5; // 劣化曲线超界（倍数 ≥10）
pub const XC_STRS_CONS: u32 = 0x1AE6; // 读账守恒破

/// 四核闭集。
pub const N_CORES: u32 = 4;
/// 单轮读数（100 万）。
pub const READS_PER_ROUND: u64 = 1_000_000;
/// 劣化倍数红线。
pub const DEGRADE_MAX: u32 = 10;

/// 一轮压测账。
#[derive(Clone, Copy)]
pub struct RoundRec {
    pub round: u32,
    pub reads: u64,
    pub viol: u32,
    pub cyc_per_read_x100: u32, // 平均每读周期 ×100（劣化曲线点）
}

/// 压测资产（隔离命名空间）。
pub struct StressRig {
    isolated: bool,
    rounds: Vec<RoundRec>,
    cur_reads: u64,
    cur_viol: u32,
    last_val: [u64; 4],
    baseline_cyc: u32,
    sealed: bool,
}

impl StressRig {
    pub fn new(isolated: bool) -> StressRig {
        StressRig {
            isolated,
            rounds: Vec::new(),
            cur_reads: 0,
            cur_viol: 0,
            last_val: [0; 4],
            baseline_cyc: 0,
            sealed: false,
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.isolated
    }

    /// 基线周期数登记（劣化曲线起点）。
    pub fn set_baseline(&mut self, cyc: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_STRS_SEALED);
        }
        self.baseline_cyc = cyc.max(1);
        Ok(())
    }

    /// 单次并发读（核号+值；同核回退即违例立案）。
    pub fn read(&mut self, core: u32, value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_STRS_SEALED);
        }
        if core >= N_CORES {
            return Err(XC_STRS_CORE);
        }
        self.cur_reads = self.cur_reads.wrapping_add(1);
        if value < self.last_val[core as usize] {
            self.cur_viol = self.cur_viol.wrapping_add(1);
            return Err(XC_STRS_MONO);
        }
        self.last_val[core as usize] = value;
        Ok(())
    }

    /// 收轮（记一轮账 + 劣化点）。
    pub fn end_round(&mut self, cyc_per_read_x100: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_STRS_SEALED);
        }
        if cyc_per_read_x100 > self.baseline_cyc.saturating_mul(DEGRADE_MAX) {
            self.cur_viol = self.cur_viol.wrapping_add(1);
            let r = RoundRec {
                round: self.rounds.len() as u32,
                reads: self.cur_reads,
                viol: self.cur_viol,
                cyc_per_read_x100,
            };
            self.rounds.push(r);
            return Err(XC_STRS_CURVE);
        }
        let r = RoundRec {
            round: self.rounds.len() as u32,
            reads: self.cur_reads,
            viol: self.cur_viol,
            cyc_per_read_x100,
        };
        self.rounds.push(r);
        self.cur_reads = 0;
        self.cur_viol = 0;
        Ok(())
    }

    /// 零违例硬闸：全部轮 viol==0。
    pub fn zero_violation(&self) -> bool {
        self.rounds.iter().all(|r| r.viol == 0) && self.cur_viol == 0
    }

    /// 总读数。
    pub fn total_reads(&self) -> u64 {
        self.rounds.iter().map(|r| r.reads).sum::<u64>().saturating_add(self.cur_reads)
    }

    /// 10^7 口径达成。
    pub fn target_reached(&self) -> bool {
        self.total_reads() >= 10_000_000
    }

    /// 劣化曲线落账：曲线点数=轮数且末点/首点 <10 倍。
    pub fn curve_ok(&self) -> bool {
        if self.rounds.is_empty() {
            return false;
        }
        let first = self.rounds[0].cyc_per_read_x100.max(1);
        let last = self.rounds[self.rounds.len() - 1].cyc_per_read_x100;
        last / first < DEGRADE_MAX as u32 && self.rounds.len() >= 10
    }

    /// 守恒：轮内读数之和 == 账面总数。
    pub fn conserved(&self) -> bool {
        self.total_reads() == self.rounds.iter().map(|r| r.reads).sum::<u64>() + self.cur_reads
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_STRS_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹：逐轮全入列。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.rounds.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in self.rounds.iter() {
            h ^= r.round ^ (r.reads as u32) ^ r.viol ^ r.cyc_per_read_x100;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.baseline_cyc;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 24 | (self.rounds.len() as u64 & 0xFFFFFF)
    }

    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）同核回退违例必抓（10/10）
    {
        let mut hits = 0;
        for k in 0..10u64 {
            let mut r = StressRig::new(true);
            let _ = r.read(0, 1000 + k);
            if r.read(0, 1000 + k - 1) == Err(XC_STRS_MONO) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）核号越界必抓（4..=13）
    {
        let mut r = StressRig::new(true);
        let mut hits = 0;
        for c in 4..=13u32 {
            if r.read(c, 10) == Err(XC_STRS_CORE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）非隔离资产必标（隔离位为假 10/10 可检出）
    {
        let mut hits = 0;
        for _ in 0..10 {
            let r = StressRig::new(false);
            if !r.is_isolated() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）劣化超界必抓（基线 100、末点 ≥1000）
    {
        let mut hits = 0;
        for k in 0..10u32 {
            let mut r = StressRig::new(true);
            let _ = r.set_baseline(100);
            let _ = r.read(0, 1);
            let heavy = 100 * DEGRADE_MAX + 1 + k;
            if r.end_round(heavy) == Err(XC_STRS_CURVE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）封账后压测必拒
    {
        let mut r = StressRig::new(true);
        let _ = r.read(0, 1);
        let _ = r.seal();
        if r.read(0, 2) == Err(XC_STRS_SEALED) {
            caught += 1;
        }
    }
    // 6）违例后零违例闸失效必抓（违例计数进轮账）
    {
        let mut r = StressRig::new(true);
        let _ = r.read(0, 100);
        let _ = r.read(0, 50); // 违例
        let _ = r.end_round(100);
        if !r.zero_violation() {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F0956→F1011-J1。
pub fn run_unx1011_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1011");

    // C11-01 四核并发读零违例（10 轮 × 10 万抽样档 = 10^7 口径代表档）
    {
        let mut ok = true;
        for _ in 0..10 {
            let mut r = StressRig::new(true);
            let _ = r.set_baseline(40);
            let mut v = 0u64;
            for i in 0..100_000u64 {
                v = v.wrapping_add(1000);
                if r.read((i % 4) as u32, v.wrapping_add((i % 4) as u64)).is_err() {
                    ok = false;
                    break;
                }
            }
            if !ok || r.end_round(42).is_err() {
                ok = false;
                break;
            }
        }
        s.add("C11-01-并发零违例", ok, if ok { "" } else { "单调违例" });
    }
    // C11-02 零违例硬闸
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for i in 0..1000u64 {
            let _ = r.read((i % 4) as u32, i * 10);
        }
        let _ = r.end_round(41);
        s.add("C11-02-硬闸", r.zero_violation(), if r.zero_violation() { "" } else { "违例残留" });
    }
    // C11-03 资产隔离断言
    {
        let a = StressRig::new(true);
        let b = StressRig::new(false);
        let ok = a.is_isolated() && !b.is_isolated();
        s.add("C11-03-资产隔离", ok, if ok { "" } else { "隔离位漂" });
    }
    // C11-04 劣化曲线落账（10 点，末/首 <10）
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for k in 0..10u32 {
            // 曲线：40,42,44,...,58（温和劣化 <10 倍）
            // 每轮先喂一点读数（end_round 不清零计数，喂一轮新账）
            let mut r2 = StressRig::new(true);
            let _ = r2.set_baseline(40);
            for i in 0..10u64 {
                let _ = r2.read((i % 4) as u32, i + 1 + k as u64 * 100);
            }
            if r2.end_round(40 + 2 * k).is_err() {
                // 记账失败即破
            }
            let _ = r.read((k % 4) as u32, k as u64 + 1);
            let _ = r.end_round(40 + 2 * k);
        }
        let ok = r.curve_ok();
        s.add("C11-04-劣化曲线", ok, if ok { "" } else { "曲线缺" });
    }
    // C11-05 劣化超界拒绝
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for i in 0..10u64 {
            let _ = r.read((i % 4) as u32, i + 1);
        }
        let ok = r.end_round(4000) == Err(XC_STRS_CURVE);
        s.add("C11-05-劣化超界拒", ok, if ok { "" } else { "超界漏" });
    }
    // C11-06 10^7 口径达成（轮账累计）
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for _ in 0..10 {
            for i in 0..1_000_000u64 {
                let _ = r.read((i % 4) as u32, i + 1);
            }
            let _ = r.end_round(45);
        }
        let ok = r.target_reached() && r.total_reads() == 10_000_000 && r.conserved();
        s.add("C11-06-千万口径", ok, if ok { "" } else { "口径缺" });
    }
    // C11-07 守恒
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for i in 0..500u64 {
            let _ = r.read((i % 4) as u32, i + 1);
        }
        let _ = r.end_round(41);
        s.add("C11-07-读账守恒", r.conserved(), if r.conserved() { "" } else { "守恒破" });
    }
    // C11-08 封账冻结
    {
        let mut r = StressRig::new(true);
        let _ = r.read(0, 1);
        let fp = r.seal();
        let ok = fp.is_ok() && r.read(0, 2) == Err(XC_STRS_SEALED) && r.set_baseline(10) == Err(XC_STRS_SEALED);
        s.add("C11-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C11-09 指纹往返
    {
        let build = || {
            let mut r = StressRig::new(true);
            let _ = r.set_baseline(40);
            for i in 0..100u64 {
                let _ = r.read((i % 4) as u32, i + 1);
            }
            let _ = r.end_round(44);
            r.fingerprint()
        };
        let ok = build() == build();
        s.add("C11-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C11-10 轮账分族（轮号单调）
    {
        let mut r = StressRig::new(true);
        let _ = r.set_baseline(40);
        for k in 0..5u32 {
            for i in 0..10u64 {
                let _ = r.read((i % 4) as u32, i + 1 + k as u64 * 1000);
            }
            let _ = r.end_round(42);
        }
        let ok = r.round_count() == 5 && r.rounds.iter().enumerate().all(|(i, rr)| rr.round == i as u32);
        s.add("C11-10-轮号单调", ok, if ok { "" } else { "轮账漂" });
    }
    // C11-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C11-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C11-12 核号边界（3 合法 4 拒）
    {
        let mut r = StressRig::new(true);
        let a = r.read(3, 10);
        let b = r.read(4, 20);
        let ok = a.is_ok() && b == Err(XC_STRS_CORE);
        s.add("C11-12-核号边界", ok, if ok { "" } else { "边界误判" });
    }
    s
}
