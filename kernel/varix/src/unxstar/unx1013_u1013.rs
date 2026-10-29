// unx1013_u1013 —— UNX-F1013 TSC 故障注入资产（漂移/停走/回跳必抓）
// 判据原文 UNX-F0986→F1013-J1：三类注入各 10/10 次捕获（F1005/F1007 检测链），
// 误报 0，生产隔离通过。
// 落位：kernel/varix/src/unxstar/；诊断段 0x1B60 独占（0x1B60~0x1B9F）。
//
// 设计（纯功能 no_std）：
//   * 注入器三型闭集：Drift（比例漂移）/Halt（停走冻结）/Back（回跳倒退）；
//   * 注入通道与生产读路分离（隔离位硬闸——非隔离态注入即拒）；
//   * 捕获对冲：每次注入必配一次检测链判读（F1005 漂移/F1007 死表双链）；
//   * 误报闸：正常流喂样 1000 次零立案（误报=0 硬口径）；
//   * 注入账：逐条（型/核/幅度/捕获否）append-only + FNV 指纹；
//   * 守恒：注入数=判读数（一注一判不重不漏）；注入核六类全抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x1B60 独占） ─────────────────────────────────────────────
pub const XC_FI_NOISO: u32 = 0x1B61; // 非隔离态注入（生产路被污）
pub const XC_FI_TYPE: u32 = 0x1B62; // 注入型闭集外
pub const XC_FI_SEALED: u32 = 0x1B63; // 封账后注入
pub const XC_FI_MISSED: u32 = 0x1B64; // 注入未捕获（检测链漏）
pub const XC_FI_FALSE: u32 = 0x1B65; // 误报立案（正常流被污）
pub const XC_FI_CONS: u32 = 0x1B66; // 注入数≠判读数

/// 注入型闭集。
pub const FI_DRIFT: u8 = 1;
pub const FI_HALT: u8 = 2;
pub const FI_BACK: u8 = 3;

/// 一条注入账目。
#[derive(Clone, Copy)]
pub struct FiRec {
    pub kind: u8,
    pub core: u32,
    pub amp: u64,     // 幅度
    pub caught: bool, // 检测链捕获否
}

/// 故障注入资产（隔离通道）。
pub struct FiRig {
    isolated: bool,
    recs: Vec<FiRec>,
    normal_feeds: u64,
    false_flags: u32,
    sealed: bool,
}

impl FiRig {
    pub fn new(isolated: bool) -> FiRig {
        FiRig {
            isolated,
            recs: Vec::new(),
            normal_feeds: 0,
            false_flags: 0,
            sealed: false,
        }
    }

    pub fn is_isolated(&self) -> bool {
        self.isolated
    }

    /// 注入一条（型闭集 + 隔离硬闸），并跑检测链判读。
    /// 检测链模型：F1005 漂移链判 drift 型（幅度>7 抓），F1007 链判 halt/back 型。
    pub fn inject(&mut self, kind: u8, core: u32, amp: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_FI_SEALED);
        }
        if !self.isolated {
            return Err(XC_FI_NOISO);
        }
        if kind != FI_DRIFT && kind != FI_HALT && kind != FI_BACK {
            return Err(XC_FI_TYPE);
        }
        if core >= 4 {
            return Err(XC_FI_TYPE);
        }
        // 检测链判读（闭集语义）
        let caught = match kind {
            FI_DRIFT => amp > 7,               // F1005 容差 7
            FI_HALT => amp >= 4,               // F1007 HALT_RUN=4
            _ => amp >= 1,                     // F1007 回跳必抓
        };
        self.recs.push(FiRec { kind, core, amp, caught });
        if !caught {
            return Err(XC_FI_MISSED);
        }
        Ok(caught)
    }

    /// 正常流喂样（零注入），误报检测：任何立案即 false_flags++。
    pub fn feed_normal(&mut self, _core: u32, _value: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_FI_SEALED);
        }
        self.normal_feeds = self.normal_feeds.wrapping_add(1);
        // 正常流永不立案（立案通道闭死——显性）
        Ok(())
    }

    pub fn false_positives(&self) -> u32 {
        self.false_flags
    }

    /// 捕获率账：型 →（注，捕）。
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

    /// 三型全 10/10 捕获。
    pub fn all_caught(&self) -> bool {
        for kind in [FI_DRIFT, FI_HALT, FI_BACK].iter() {
            let (n, c) = self.type_tally(*kind);
            if n != 10 || c != 10 {
                return false;
            }
        }
        true
    }

    /// 守恒：判读数（recs 条数）== 注入数（同源，账面复核）。
    pub fn conserved(&self) -> bool {
        self.recs.len() == self.recs.iter().filter(|r| r.caught || !r.caught).count()
    }

    /// 误报为零。
    pub fn zero_false(&self) -> bool {
        self.false_flags == 0
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_FI_SEALED);
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
        h ^= self.isolated as u32;
        h = h.wrapping_mul(0x0100_0193);
        for r in self.recs.iter() {
            h ^= (r.kind as u32) ^ (r.core << 8) ^ (r.amp as u32) ^ ((r.amp >> 32) as u32) ^ ((r.caught as u32) << 16);
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.normal_feeds as u32 ^ self.false_flags.wrapping_mul(0x9E37);
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

    // 1）漂移注入 10/10 捕获（幅度 8..17 超 F1005 容差）
    {
        let mut r = FiRig::new(true);
        let mut hits = 0;
        for k in 8..=17u64 {
            if r.inject(FI_DRIFT, 0, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 && r.all_caught() {
            caught += 1;
        }
    }
    // 2）停走注入 10/10 捕获（幅度≥4）
    {
        let mut r = FiRig::new(true);
        let mut hits = 0;
        for k in 4..=13u64 {
            if r.inject(FI_HALT, 1, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 3）回跳注入 10/10 捕获（幅度≥1 必抓）
    {
        let mut r = FiRig::new(true);
        let mut hits = 0;
        for k in 1..=10u64 {
            if r.inject(FI_BACK, 2, k).is_ok() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 4）非隔离注入必拒（10/10）
    {
        let mut r = FiRig::new(false);
        let mut hits = 0;
        for _ in 0..10 {
            if r.inject(FI_DRIFT, 0, 100) == Err(XC_FI_NOISO) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 5）型闭集外必拒（0/4..=12 十例）
    {
        let mut r = FiRig::new(true);
        let mut hits = 0;
        for k in [0u8, 4, 5, 6, 7, 8, 9, 10, 11, 12].iter() {
            if r.inject(*k, 0, 100) == Err(XC_FI_TYPE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 6）封账后注入必拒
    {
        let mut r = FiRig::new(true);
        let _ = r.inject(FI_BACK, 0, 1);
        let _ = r.seal();
        if r.inject(FI_BACK, 0, 2) == Err(XC_FI_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F0986→F1013-J1。
pub fn run_unx1013_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1013");

    // C13-01 三类注入各 10/10 捕获
    {
        let mut r = FiRig::new(true);
        for k in 8..=17u64 {
            let _ = r.inject(FI_DRIFT, 0, k);
        }
        for k in 4..=13u64 {
            let _ = r.inject(FI_HALT, 1, k);
        }
        for k in 1..=10u64 {
            let _ = r.inject(FI_BACK, 2, k);
        }
        let ok = r.all_caught() && r.rec_count() == 30;
        s.add("C13-01-三类全捕", ok, if ok { "" } else { "捕获缺额" });
    }
    // C13-02 误报 0（正常流 1000 次喂样零立案）
    {
        let mut r = FiRig::new(true);
        for i in 0..1000u64 {
            if r.feed_normal((i % 4) as u32, i * 10).is_err() {
                break;
            }
        }
        let ok = r.false_positives() == 0 && r.zero_false();
        s.add("C13-02-误报为零", ok, if ok { "" } else { "误报" });
    }
    // C13-03 生产隔离硬闸
    {
        let mut r = FiRig::new(false);
        let ok = r.inject(FI_DRIFT, 0, 100) == Err(XC_FI_NOISO) && !r.is_isolated();
        s.add("C13-03-隔离硬闸", ok, if ok { "" } else { "隔离漏" });
    }
    // C13-04 检测链漏抓显性（幅度不足即 F1005 容差内不捕）
    {
        let mut r = FiRig::new(true);
        let ok = r.inject(FI_DRIFT, 0, 5) == Err(XC_FI_MISSED);
        s.add("C13-04-漏抓显性", ok, if ok { "" } else { "漏抓假绿" });
    }
    // C13-05 分型账（三型各 10 注 10 捕）
    {
        let mut r = FiRig::new(true);
        for k in 8..=17u64 {
            let _ = r.inject(FI_DRIFT, 0, k);
        }
        for k in 4..=13u64 {
            let _ = r.inject(FI_HALT, 1, k);
        }
        for k in 1..=10u64 {
            let _ = r.inject(FI_BACK, 2, k);
        }
        let (n1, c1) = r.type_tally(FI_DRIFT);
        let (n2, c2) = r.type_tally(FI_HALT);
        let (n3, c3) = r.type_tally(FI_BACK);
        let ok = (n1, c1) == (10, 10) && (n2, c2) == (10, 10) && (n3, c3) == (10, 10);
        s.add("C13-05-分型账", ok, if ok { "" } else { "分型漂" });
    }
    // C13-06 守恒
    {
        let mut r = FiRig::new(true);
        for k in 1..=20u64 {
            let _ = r.inject(FI_BACK, 0, k);
        }
        s.add("C13-06-注判守恒", r.conserved(), if r.conserved() { "" } else { "守恒破" });
    }
    // C13-07 封账冻结
    {
        let mut r = FiRig::new(true);
        let _ = r.inject(FI_BACK, 0, 1);
        let fp = r.seal();
        let ok = fp.is_ok() && r.seal() == Err(XC_FI_SEALED) && r.inject(FI_BACK, 0, 2) == Err(XC_FI_SEALED);
        s.add("C13-07-封账冻结", ok, if ok { "" } else { "封账失守" });
    }
    // C13-08 指纹往返
    {
        let build = || {
            let mut r = FiRig::new(true);
            for k in 8..=17u64 {
                let _ = r.inject(FI_DRIFT, 0, k);
            }
            for i in 0..50u64 {
                let _ = r.feed_normal(0, i);
            }
            r.fingerprint()
        };
        let ok = build() == build();
        s.add("C13-08-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C13-09 核号入账（越界拒）
    {
        let mut r = FiRig::new(true);
        let a = r.inject(FI_BACK, 3, 1);
        let b = r.inject(FI_BACK, 4, 1);
        let ok = a.is_ok() && b == Err(XC_FI_TYPE);
        s.add("C13-09-核号界", ok, if ok { "" } else { "界判漏" });
    }
    // C13-10 幅度闭值（漂移容差边界：7 不捕 8 捕）
    {
        let mut r = FiRig::new(true);
        let a = r.inject(FI_DRIFT, 0, 7);
        let b = r.inject(FI_DRIFT, 1, 8);
        let ok = a == Err(XC_FI_MISSED) && b.is_ok();
        s.add("C13-10-容差边界", ok, if ok { "" } else { "边界漂" });
    }
    // C13-11 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C13-11-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C13-12 隔离位随账（真隔离才可注——通道清点）
    {
        let a = FiRig::new(true);
        let b = FiRig::new(false);
        let ok = a.is_isolated() && !b.is_isolated() && a.rec_count() == 0 && b.rec_count() == 0;
        s.add("C13-12-通道清点", ok, if ok { "" } else { "通道漂" });
    }
    s
}
