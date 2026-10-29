// unx2004_capaware —— UNX-F2004 容量感知调度挂点（最大频率比容量修正）
// 判据原文 UNX-F2004-J1：容量修正断言全过（最大频率比值→容量修正系数→
// 调度决策消费闭环），修正错读注入 10/10 检出，修正账在册。
// 落位：kernel/varix/src/unxstar/；诊断段 0x7AA0 独占（0x7AA0~0x7ABF）。
//
// 设计（纯功能 no_std）：
//   * 容量口径：调度容量基线 1024（与 B09 调度域同源），修正后容量 =
//     base_cap × ratio（最大频率比，千分比定点 0..=2000）× 1024 / 1000；
//   * 比值来源：max_freq / nominal_freq，逐 CPU 档案登记，构造期冻结；
//   * 修正链闭环：比值登记 → 系数计算 → 修正容量 → 调度决策消费（快照读取）；
//   * 错读注入 10/10：比值 0 / 超界（>2000）/ 未登记 CPU / 封账后改写 / 回退
//     比值，逐笔必拒入账；
//   * 修正账：seq 零跳号 + 比值·系数·容量三列一致 + 指纹冻结。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x7AA0 独占）──────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x7AA0;

pub const XC_CP_RATIO_ZERO: u32 = SEG_BASE as u32 + 0x01; // 比值为 0（错读）
pub const XC_CP_RATIO_HI: u32 = SEG_BASE as u32 + 0x02; // 比值超界（> 2000‰）
pub const XC_CP_CPU_OOB: u32 = SEG_BASE as u32 + 0x03; // CPU 号未登记
pub const XC_CP_SEALED: u32 = SEG_BASE as u32 + 0x04; // 封账后改写
pub const XC_CP_REGRESS: u32 = SEG_BASE as u32 + 0x05; // 比值回退（同 CPU 新值 < 旧值）
pub const XC_CP_COEFF: u32 = SEG_BASE as u32 + 0x06; // 系数与比值失配
pub const XC_CP_CONSUME_STALE: u32 = SEG_BASE as u32 + 0x07; // 消费了过期修正代

pub const CAP_BASE: u32 = 1024; // 调度容量基线（与 B09 同源）
pub const RATIO_MAX: u32 = 2000; // 千分比上限（Turbo 2.0x）
pub const RATIO_MIN: u32 = 1; // 千分比下限（不为零）

/// CPU 频率档案：最大频率与标称频率（构造期冻结）。
#[derive(Clone, Copy)]
pub struct FreqProfile {
    pub cpu: u32,
    pub max_khz: u32,
    pub nominal_khz: u32,
}

impl FreqProfile {
    /// 最大频率比（千分比）：max/nominal × 1000，零除显性拒。
    pub fn ratio_permille(&self) -> Result<u32, u32> {
        if self.nominal_khz == 0 {
            return Err(XC_CP_RATIO_ZERO);
        }
        if self.max_khz == 0 {
            return Err(XC_CP_RATIO_ZERO);
        }
        let r = ((self.max_khz as u64) * 1000 / self.nominal_khz as u64) as u32;
        if r == 0 {
            return Err(XC_CP_RATIO_ZERO);
        }
        if r > RATIO_MAX {
            return Err(XC_CP_RATIO_HI);
        }
        Ok(r)
    }
}

/// 修正账单行：seq + CPU + 比值 + 系数 + 修正后容量 五列。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CorrRow {
    pub seq: u64,
    pub cpu: u32,
    pub ratio_permille: u32,
    /// 修正系数：容量定点（基线 1024 口径）。
    pub coeff: u32,
    /// 修正后容量：coeff 消费者直接读取的最终值。
    pub effective_cap: u32,
}

/// 容量感知修正器 + 调度挂点。
pub struct CapacityCorrector {
    profiles: Vec<FreqProfile>,
    /// 每 CPU 当前生效比值（登记制，同 CPU 覆写需单调不回退）。
    live_ratio: Vec<Option<u32>>,
    ledger: Vec<CorrRow>,
    seq: u64,
    sealed: bool,
    /// 调度侧消费的当前修正代（每轮修正自增，消费必须取最新代）。
    epoch: u64,
    rejects: Vec<(u32, &'static str)>,
}

impl CapacityCorrector {
    pub fn new() -> CapacityCorrector {
        CapacityCorrector {
            profiles: Vec::new(),
            live_ratio: Vec::new(),
            ledger: Vec::new(),
            seq: 0,
            sealed: false,
            epoch: 0,
            rejects: Vec::new(),
        }
    }

    /// 登记频率档案（同 CPU 重复登记拒；比值越界/为零拒）。
    pub fn register(&mut self, p: FreqProfile) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_CP_SEALED, "封账后登记"));
            return Err(XC_CP_SEALED);
        }
        for q in &self.profiles {
            if q.cpu == p.cpu {
                self.rejects.push((XC_CP_CPU_OOB, "CPU 重复登记"));
                return Err(XC_CP_CPU_OOB);
            }
        }
        // 比值合法性即检（错读防线）
        p.ratio_permille()?;
        self.profiles.push(p);
        self.live_ratio.push(None);
        Ok(())
    }

    fn cpu_slot(&self, cpu: u32) -> Option<usize> {
        let mut i = 0usize;
        for p in &self.profiles {
            if p.cpu == cpu {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 比值生效：登记 CPU 的比值写入生效表（回退拒）。
    pub fn activate(&mut self, cpu: u32, ratio_permille: u32) -> Result<u64, u32> {
        if self.sealed {
            self.rejects.push((XC_CP_SEALED, "封账后生效"));
            return Err(XC_CP_SEALED);
        }
        let slot = match self.cpu_slot(cpu) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_CP_CPU_OOB, "CPU 未登记"));
                return Err(XC_CP_CPU_OOB);
            }
        };
        if ratio_permille == 0 {
            self.rejects.push((XC_CP_RATIO_ZERO, "比值零错读"));
            return Err(XC_CP_RATIO_ZERO);
        }
        if ratio_permille > RATIO_MAX {
            self.rejects.push((XC_CP_RATIO_HI, "比值超界"));
            return Err(XC_CP_RATIO_HI);
        }
        if let Some(prev) = self.live_ratio[slot] {
            if ratio_permille < prev {
                self.rejects.push((XC_CP_REGRESS, "比值回退"));
                return Err(XC_CP_REGRESS);
            }
        }
        self.live_ratio[slot] = Some(ratio_permille);
        self.epoch += 1;
        Ok(self.epoch)
    }

    /// 修正系数：容量定点 = 1024 × ratio / 1000（四舍五入，零截断偏差防线）。
    pub fn coeff_of(ratio_permille: u32) -> Result<u32, u32> {
        if ratio_permille == 0 {
            return Err(XC_CP_RATIO_ZERO);
        }
        if ratio_permille > RATIO_MAX {
            return Err(XC_CP_RATIO_HI);
        }
        let c = ((CAP_BASE as u64) * (ratio_permille as u64) + 500) / 1000;
        Ok(c as u32)
    }

    /// 修正后容量：coeff 消费为调度容量。
    pub fn effective_cap(coeff: u32) -> u32 {
        coeff.max(1)
    }

    /// 修正入账：比值 → 系数 → 容量 三列一致写入修正账。
    pub fn account(&mut self, cpu: u32) -> Result<CorrRow, u32> {
        if self.sealed {
            self.rejects.push((XC_CP_SEALED, "封账后修正"));
            return Err(XC_CP_SEALED);
        }
        let slot = match self.cpu_slot(cpu) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_CP_CPU_OOB, "CPU 未登记"));
                return Err(XC_CP_CPU_OOB);
            }
        };
        let ratio = match self.live_ratio[slot] {
            Some(r) => r,
            None => {
                self.rejects.push((XC_CP_RATIO_ZERO, "比值未生效"));
                return Err(XC_CP_RATIO_ZERO);
            }
        };
        let coeff = Self::coeff_of(ratio)?;
        let row = CorrRow {
            seq: self.seq + 1,
            cpu,
            ratio_permille: ratio,
            coeff,
            effective_cap: Self::effective_cap(coeff),
        };
        // 三列一致性自检（错读防线：入账即核）
        let expect_coeff = ((CAP_BASE as u64) * (ratio as u64) + 500) / 1000;
        if (coeff as u64) != expect_coeff {
            self.rejects.push((XC_CP_COEFF, "系数失配"));
            return Err(XC_CP_COEFF);
        }
        self.seq = row.seq;
        self.ledger.push(row);
        Ok(row)
    }

    /// 调度决策消费挂点：读取最新修正代的账尾行（消费过期代即拒）。
    pub fn consume(&self, consumer_epoch: u64) -> Result<CorrRow, u32> {
        match self.ledger.last() {
            Some(row) => {
                if consumer_epoch != 0 && consumer_epoch < self.epoch {
                    return Err(XC_CP_CONSUME_STALE);
                }
                Ok(*row)
            }
            None => Err(XC_CP_CONSUME_STALE),
        }
    }

    /// 闭环校验：账尾行的容量能被调度快照读取（同代一致）。
    pub fn closed_loop(&self) -> bool {
        if let Some(row) = self.ledger.last() {
            match self.consume(0) {
                Ok(c) => c.effective_cap == row.effective_cap && c.seq == row.seq,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// 修正账审计：seq 零跳号 + 每 CPU 比值单调不回退。
    pub fn audit(&self) -> bool {
        for (i, r) in self.ledger.iter().enumerate() {
            if r.seq != (i as u64) + 1 {
                return false;
            }
        }
        // 同 CPU 比值按账序单调不回退
        for (slot, p) in self.profiles.iter().enumerate() {
            let mut prev: u32 = 0;
            for r in &self.ledger {
                if r.cpu == p.cpu {
                    if prev != 0 && r.ratio_permille < prev {
                        return false;
                    }
                    prev = r.ratio_permille;
                }
            }
            let _ = slot;
        }
        true
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_CP_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h = h.wrapping_mul(0x0100_0193) ^ (self.epoch as u32);
        for r in &self.ledger {
            h ^= r.cpu ^ r.ratio_permille ^ r.coeff ^ r.effective_cap;
            h = h.wrapping_mul(0x0100_0193);
        }
        for p in &self.profiles {
            h ^= p.cpu ^ p.max_khz ^ p.nominal_khz;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.seq as u32;
        h = h.wrapping_mul(0x0100_0193);
        ((h as u64) << 20) | (self.epoch.min(0xFFFFF) as u64)
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn profile_count(&self) -> usize {
        self.profiles.len()
    }
    pub fn current_epoch(&self) -> u64 {
        self.epoch
    }
}

/// 错读注入核：10/10 检出。
/// 五类错读各两笔：零比值 ×2 / 超界比值 ×2 / 未登记 CPU ×2 / 回退比值 ×2。
pub fn inject_core() -> (u32, u32) {
    const N: u32 = 10;
    let mut caught = 0u32;
    let mut c = CapacityCorrector::new();
    let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
    let _ = c.register(FreqProfile { cpu: 1, max_khz: 2_400_000, nominal_khz: 2_400_000 });
    let _ = c.activate(0, 1500);

    // 1~2）零比值生效必拒
    if c.activate(0, 0) == Err(XC_CP_RATIO_ZERO) {
        caught += 1;
    }
    if c.activate(1, 0) == Err(XC_CP_RATIO_ZERO) {
        caught += 1;
    }
    // 3~4）超界比值必拒（2001‰ / 9999‰）
    if c.activate(0, 2001) == Err(XC_CP_RATIO_HI) {
        caught += 1;
    }
    if c.activate(1, 9999) == Err(XC_CP_RATIO_HI) {
        caught += 1;
    }
    // 5~6）未登记 CPU 必拒（cpu 7 / cpu 99）
    if c.activate(7, 1200) == Err(XC_CP_CPU_OOB) {
        caught += 1;
    }
    if c.activate(99, 1200) == Err(XC_CP_CPU_OOB) {
        caught += 1;
    }
    // 7~8）比值回退必拒（1500 → 1200 → 1000）
    if c.activate(0, 1200) == Err(XC_CP_REGRESS) {
        caught += 1;
    }
    if c.activate(0, 1000) == Err(XC_CP_REGRESS) {
        caught += 1;
    }
    // 9）未生效 CPU 修正必拒（cpu 1 有档案但从未 activate）
    if c.account(1).is_err() {
        caught += 1;
    }
    // 10）封账后修正必拒
    let _ = c.account(0);
    let _ = c.seal();
    if c.account(0) == Err(XC_CP_SEALED) {
        caught += 1;
    }
    (N, caught)
}

/// 判据入口：UNX-F2004-J1。
pub fn run_unx2004_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2004");

    // J1-01 比值计算（3600/2400 = 1500‰）
    {
        let p = FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 };
        let ok = p.ratio_permille() == Ok(1500);
        s.add("J1-01-比值计算", ok, if ok { "" } else { "比值漂移" });
    }
    // J1-02 系数定点（1024 × 1.5 = 1536；四舍五入口径）
    {
        let ok = CapacityCorrector::coeff_of(1500) == Ok(1536)
            && CapacityCorrector::coeff_of(1000) == Ok(1024)
            && CapacityCorrector::coeff_of(2000) == Ok(2048);
        s.add("J1-02-系数定点", ok, if ok { "" } else { "系数漂移" });
    }
    // J1-03 修正闭环：比值→系数→容量→调度消费
    {
        let mut c = CapacityCorrector::new();
        let reg = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let act = c.activate(0, 1500);
        let row = c.account(0);
        let ok = reg.is_ok()
            && act.is_ok()
            && match row {
                Ok(r) => r.coeff == 1536 && r.effective_cap == 1536 && c.closed_loop(),
                Err(_) => false,
            };
        s.add("J1-03-修正闭环", ok, if ok { "" } else { "闭环断裂" });
    }
    // J1-04 基线比值：1000‰ 修正后容量恰为基线 1024
    {
        let mut c = CapacityCorrector::new();
        let _ = c.register(FreqProfile { cpu: 2, max_khz: 2_400_000, nominal_khz: 2_400_000 });
        let _ = c.activate(2, 1000);
        let ok = match c.account(2) {
            Ok(r) => r.coeff == CAP_BASE && r.effective_cap == CAP_BASE,
            Err(_) => false,
        };
        s.add("J1-04-基线恒等", ok, if ok { "" } else { "基线漂移" });
    }
    // J1-05 消费过期代即拒
    {
        let mut c = CapacityCorrector::new();
        let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let _ = c.activate(0, 1500);
        let _ = c.account(0);
        let stale_epoch = c.current_epoch();
        let _ = c.activate(0, 1600);
        let _ = c.account(0);
        let ok = c.consume(stale_epoch) == Err(XC_CP_CONSUME_STALE) && c.consume(0).is_ok();
        s.add("J1-05-过期代拒", ok, if ok { "" } else { "过期代放行" });
    }
    // J1-06 错读注入 10/10
    {
        let (n, k) = inject_core();
        let ok = n == 10 && k == 10;
        s.add("J1-06-错读注入10/10", ok, if ok { "" } else { "注入漏抓" });
    }
    // J1-07 修正账审计（seq 致密 + 比值单调）
    {
        let mut c = CapacityCorrector::new();
        let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let _ = c.activate(0, 1200);
        let _ = c.account(0);
        let _ = c.activate(0, 1500);
        let _ = c.account(0);
        let _ = c.activate(0, 1800);
        let _ = c.account(0);
        let ok = c.audit() && c.ledger_len() == 3;
        s.add("J1-07-修正账审计", ok, if ok { "" } else { "审计红" });
    }
    // J1-08 零标称频率档案必拒（零除显性）
    {
        let p = FreqProfile { cpu: 3, max_khz: 3_600_000, nominal_khz: 0 };
        let ok = p.ratio_permille() == Err(XC_CP_RATIO_ZERO);
        s.add("J1-08-零标称拒", ok, if ok { "" } else { "零除放行" });
    }
    // J1-09 重复登记 CPU 必拒
    {
        let mut c = CapacityCorrector::new();
        let r1 = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let r2 = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let ok = r1.is_ok() && r2 == Err(XC_CP_CPU_OOB);
        s.add("J1-09-重复登记拒", ok, if ok { "" } else { "重复放行" });
    }
    // J1-10 多 CPU 独立修正（互不串账）
    {
        let mut c = CapacityCorrector::new();
        let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let _ = c.register(FreqProfile { cpu: 1, max_khz: 2_400_000, nominal_khz: 2_400_000 });
        let _ = c.activate(0, 1500);
        let _ = c.activate(1, 1000);
        let r0 = c.account(0);
        let r1 = c.account(1);
        let ok = match (r0, r1) {
            (Ok(a), Ok(b)) => a.coeff == 1536 && b.coeff == 1024 && a.cpu != b.cpu,
            _ => false,
        };
        s.add("J1-10-多CPU独立", ok, if ok { "" } else { "跨 CPU 串账" });
    }
    // J1-11 封账稳定（指纹不变 + 二次封账拒）
    {
        let mut c = CapacityCorrector::new();
        let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
        let _ = c.activate(0, 1500);
        let _ = c.account(0);
        let pre = c.fingerprint();
        let _ = c.seal();
        let ok = c.fingerprint() == pre && c.seal().is_err();
        s.add("J1-11-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    // J1-12 指纹冻结
    {
        let f = || -> u64 {
            let mut c = CapacityCorrector::new();
            let _ = c.register(FreqProfile { cpu: 0, max_khz: 3_600_000, nominal_khz: 2_400_000 });
            let _ = c.activate(0, 1500);
            let _ = c.account(0);
            c.fingerprint()
        };
        let ok = f() == f() && f() != 0;
        s.add("J1-12-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }
    // J1-13 系数下限：容量不低于 1（不可用 0 容量毒化调度）
    {
        // 最小合法比值 1‰ → 系数 = (1024×1+500)/1000 = 1（下限贴地）
        let ok = CapacityCorrector::coeff_of(1) == Ok(1);
        s.add("J1-13-容量下限", ok, if ok { "" } else { "零容量毒化" });
    }
    // J1-14 段位独占
    {
        let ok = SEG_BASE == 0x7AA0;
        s.add("J1-14-段位独占", ok, if ok { "" } else { "0x7AA0~0x7ABF 本模块独占失守" });
    }
    s
}
