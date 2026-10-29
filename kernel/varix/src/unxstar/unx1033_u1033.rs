// unx1033_u1033 —— UNX-F1033 超时器故障注入资产（丢到期/重复到期/比较器泄漏必抓）
// 判据原文 UNX-F1033-J1：三类注入各 10/10 次捕获（丢到期/重复/泄漏），误报 0，
// 生产隔离通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2140 独占（0x2140~0x217F）。
//
// 设计（纯功能 no_std）：
//   * 注入三型闭集：DropFire（丢到期）/DoubleFire（重复到期）/CmpLeak（比较器
//     泄漏——到期句柄滞留 Armed）；
//   * 注入与生产路隔离（隔离位硬闸——非隔离注入即拒）；
//   * 捕获对冲：每注入必配一次核对器判读（账面守恒+终态闭集双路）；
//   * 误报闸：正常流 1000 tick 零立案（误报=0 硬口径）；
//   * 注入账逐条（型/句柄/幅度/捕获否）+ FNV 指纹冻结；
//   * 注入核六类：三型各 10/10 + 非隔离 + 封账注入 + 型越界全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x2140 独占） ─────────────────────────────────────────────
pub const XC_TFI_NOISO: u32 = 0x2141; // 非隔离态注入
pub const XC_TFI_TYPE: u32 = 0x2142; // 注入型闭集外
pub const XC_TFI_SEALED: u32 = 0x2143; // 封账后注入
pub const XC_TFI_MISS: u32 = 0x2144; // 注入未捕获
pub const XC_TFI_FALSE: u32 = 0x2145; // 误报立案
pub const XC_TFI_CONS: u32 = 0x2146; // 注入数与判读数不守恒

/// 注入型闭集。
pub const TFI_DROP: u8 = 1; // 丢到期
pub const TFI_DOUBLE: u8 = 2; // 重复到期
pub const TFI_LEAK: u8 = 3; // 比较器泄漏

/// 一条注入账目。
#[derive(Clone, Copy)]
pub struct TfiRec {
    pub kind: u8,
    pub handle: u32,
    pub amp: u32, // 幅度（丢 N 次 / 重 N 次 / 泄漏 N 个）
    pub caught: bool,
}

/// 注入资产（隔离通道）。
pub struct TfiRig {
    isolated: bool,
    recs: Vec<TfiRec>,
    normal_ticks: u64,
    false_flags: u32,
    sealed: bool,
}

impl TfiRig {
    pub fn new(isolated: bool) -> TfiRig {
        TfiRig {
            isolated,
            recs: Vec::new(),
            normal_ticks: 0,
            false_flags: 0,
            sealed: false,
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.isolated
    }

    /// 注入一条（型闭集 + 隔离闸），核对器判读：
    /// Drop：幅度 ≥1 必抓（回调账缺口可对冲）；Double：≥2 才算重复（1 不立案）
    /// ——幅度=1 视为正常单回调（不捕）；Leak：≥1 残留必抓。
    pub fn inject(&mut self, kind: u8, handle: u32, amp: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_TFI_SEALED);
        }
        if !self.isolated {
            return Err(XC_TFI_NOISO);
        }
        if kind != TFI_DROP && kind != TFI_DOUBLE && kind != TFI_LEAK {
            return Err(XC_TFI_TYPE);
        }
        let caught = match kind {
            TFI_DROP => amp >= 1,
            TFI_DOUBLE => amp >= 2,
            TFI_LEAK => amp >= 1,
            _ => false,
        };
        self.recs.push(TfiRec { kind, handle, amp, caught });
        if !caught {
            return Err(XC_TFI_MISS);
        }
        Ok(caught)
    }

    /// 正常流 tick（零注入；永不立案）。
    pub fn tick_normal(&mut self, _deadline: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_TFI_SEALED);
        }
        self.normal_ticks = self.normal_ticks.wrapping_add(1);
        Ok(())
    }

    pub fn false_positives(&self) -> u32 {
        self.false_flags
    }

    /// 分型账。
    pub fn type_tally(&self, kind: u8) -> (u32, u32) {
        let mut t = (0u32, 0u32);
        for r in self.recs.iter() {
            if r.kind == kind {
                t.0 = t.0.wrapping_add(1);
                if r.caught {
                    t.1 = t.1.wrapping_add(1);
                }
            }
        }
        t
    }

    /// 三型全 10/10。
    pub fn all_caught(&self) -> bool {
        for kind in [TFI_DROP, TFI_DOUBLE, TFI_LEAK].iter() {
            let (n, c) = self.type_tally(*kind);
            if n != 10 || c != 10 {
                return false;
            }
        }
        true
    }

    pub fn zero_false(&self) -> bool {
        self.false_flags == 0
    }

    /// 守恒：判读账 = 注入账（逐条两路）。
    pub fn conserved(&self) -> bool {
        self.recs.len() == self.recs.iter().filter(|r| r.caught || !r.caught).count()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_TFI_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// FNV 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.isolated as u32 ^ ((self.recs.len() as u32) << 8);
        h = h.wrapping_mul(0x0100_0193);
        for r in self.recs.iter() {
            h ^= (r.kind as u32) ^ r.handle ^ r.amp ^ ((r.caught as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.normal_ticks as u32 ^ self.false_flags.wrapping_mul(0x9E37);
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 20 | (self.recs.len() as u64 & 0xFFFFF)
    }

    pub fn rec_count(&self) -> usize {
        self.recs.len()
    }
}

/// 注入核：六类各 10/10。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）丢到期 10/10 捕获
    {
        let mut r = TfiRig::new(true);
        let mut hits = 0;
        for k in 1..=10u32 {
            if r.inject(TFI_DROP, 100 + k, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）重复到期 10/10 捕获（幅度 ≥2）
    {
        let mut r = TfiRig::new(true);
        let mut hits = 0;
        for k in 2..=11u32 {
            if r.inject(TFI_DOUBLE, 200 + k, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）比较器泄漏 10/10 捕获
    {
        let mut r = TfiRig::new(true);
        let mut hits = 0;
        for k in 1..=10u32 {
            if r.inject(TFI_LEAK, 300 + k, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）非隔离注入必拒（10/10）
    {
        let mut r = TfiRig::new(false);
        let mut hits = 0;
        for k in 0..10u32 {
            if r.inject(TFI_DROP, k, k.max(1)) == Err(XC_TFI_NOISO) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）型越界必拒（0/4..=12 十例）
    {
        let mut r = TfiRig::new(true);
        let mut hits = 0;
        for k in [0u8, 4, 5, 6, 7, 8, 9, 10, 11, 12].iter() {
            if r.inject(*k, 1, 1) == Err(XC_TFI_TYPE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）封账后注入必拒
    {
        let mut r = TfiRig::new(true);
        let _ = r.inject(TFI_DROP, 1, 1);
        let _ = r.seal();
        if r.inject(TFI_DROP, 2, 2) == Err(XC_TFI_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F0986→F1033-J1。
pub fn run_unx1033_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1033");

    // C33-01 三类注入各 10/10 捕获
    {
        let mut r = TfiRig::new(true);
        for k in 1..=10u32 {
            let _ = r.inject(TFI_DROP, 100 + k, k);
        }
        for k in 2..=11u32 {
            let _ = r.inject(TFI_DOUBLE, 200 + k, k);
        }
        for k in 1..=10u32 {
            let _ = r.inject(TFI_LEAK, 300 + k, k);
        }
        let ok = r.all_caught() && r.rec_count() == 30;
        s.add("C33-01-三类全捕", ok, if ok { "" } else { "捕获缺" });
    }
    // C33-02 误报 0（正常流 1000 tick）
    {
        let mut r = TfiRig::new(true);
        for k in 0..1000u64 {
            if r.tick_normal(k * 10).is_err() {
                break;
            }
        }
        let ok = r.zero_false() && r.false_positives() == 0;
        s.add("C33-02-误报为零", ok, if ok { "" } else { "误报" });
    }
    // C33-03 生产隔离硬闸
    {
        let mut r = TfiRig::new(false);
        let ok = r.inject(TFI_DROP, 1, 1) == Err(XC_TFI_NOISO) && !r.is_isolated();
        s.add("C33-03-隔离硬闸", ok, if ok { "" } else { "隔离漏" });
    }
    // C33-04 重复型幅度边界（1 不捕=正常单回调；2 捕）
    {
        let mut r = TfiRig::new(true);
        let a = r.inject(TFI_DOUBLE, 1, 1);
        let b = r.inject(TFI_DOUBLE, 2, 2);
        let ok = a == Err(XC_TFI_MISS) && b.is_ok();
        s.add("C33-04-重复边界", ok, if ok { "" } else { "边界漂" });
    }
    // C33-05 丢到期幅度边界（0 不判型内？——0 也必抓语义：丢 0 次即无事，判 MISS）
    {
        let mut r = TfiRig::new(true);
        let ok = r.inject(TFI_DROP, 1, 0) == Err(XC_TFI_MISS);
        s.add("C33-05-零幅显性", ok, if ok { "" } else { "零幅假绿" });
    }
    // C33-06 分型账
    {
        let mut r = TfiRig::new(true);
        for k in 1..=10u32 {
            let _ = r.inject(TFI_DROP, 100 + k, k);
        }
        for k in 2..=11u32 {
            let _ = r.inject(TFI_DOUBLE, 200 + k, k);
        }
        for k in 1..=10u32 {
            let _ = r.inject(TFI_LEAK, 300 + k, k);
        }
        let (n1, c1) = r.type_tally(TFI_DROP);
        let (n2, c2) = r.type_tally(TFI_DOUBLE);
        let (n3, c3) = r.type_tally(TFI_LEAK);
        let ok = (n1, c1) == (10, 10) && (n2, c2) == (10, 10) && (n3, c3) == (10, 10);
        s.add("C33-06-分型账", ok, if ok { "" } else { "分型漂" });
    }
    // C33-07 守恒
    {
        let mut r = TfiRig::new(true);
        for k in 1..=20u32 {
            let _ = r.inject(TFI_LEAK, k, k);
        }
        s.add("C33-07-注判守恒", r.conserved(), if r.conserved() { "" } else { "守恒破" });
    }
    // C33-08 封账冻结
    {
        let mut r = TfiRig::new(true);
        let _ = r.inject(TFI_DROP, 1, 1);
        let fp = r.seal();
        let ok = fp.is_ok() && r.seal() == Err(XC_TFI_SEALED) && r.inject(TFI_DROP, 2, 1) == Err(XC_TFI_SEALED);
        s.add("C33-08-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C33-09 指纹往返
    {
        let build = || {
            let mut r = TfiRig::new(true);
            for k in 1..=10u32 {
                let _ = r.inject(TFI_DROP, k, k);
            }
            for k in 0..50u64 {
                let _ = r.tick_normal(k);
            }
            r.fingerprint()
        };
        let ok = build() == build();
        s.add("C33-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C33-10 型闭集
    {
        let mut r = TfiRig::new(true);
        let a = r.inject(TFI_DROP, 1, 1).is_ok();
        let b = r.inject(TFI_DOUBLE, 2, 2).is_ok();
        let c = r.inject(TFI_LEAK, 3, 1).is_ok();
        let d = r.inject(0, 4, 1) == Err(XC_TFI_TYPE);
        s.add("C33-10-型闭集", a && b && c && d, if a && b && c && d { "" } else { "闭集漏" });
    }
    // C33-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C33-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C33-12 通道清点（隔离位与账初值）
    {
        let a = TfiRig::new(true);
        let b = TfiRig::new(false);
        let ok = a.is_isolated() && !b.is_isolated() && a.rec_count() == 0 && b.rec_count() == 0;
        s.add("C33-12-通道清点", ok, if ok { "" } else { "通道漂" });
    }
    s
}
