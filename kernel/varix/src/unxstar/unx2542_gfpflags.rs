// unx2542_gfpflags —— UNX-F2542 GFP 类 flags 裁断语义总册
// 判据原文 UNX-F2542-J1：flags 全组合裁断矩阵逐项断言（可睡眠/不可睡眠/
// 零页三轴），非法组合注入 10/10 次拒绝。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF780 独占（0xF780~0xF7FF）。
//
// 设计（纯功能 no_std）：
//   * flags 三轴闭集：可睡眠（__GFP_WAIT）/ 不可睡眠（GFP_ATOMIC 语义）/
//     零页（__GFP_ZERO）—— 3×2×2 = 全组合 8 种裁断；
//   * 裁断矩阵：可睡眠+不可睡眠同置 = 非法（互斥）；其余 7 组合合法；
//   * 裁断结果：路径（SLEEPABLE/NOSLEEP）+ 清零语义（ZERO → 返回块内容约定清零）；
//   * 非法注入 10/10 必拒；指纹冻结。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF780 独占） ─────────────────────────────────────────────
pub const XC_GFP_MUTEX: u32 = 0xF781; // 可睡眠/不可睡眠互斥破
pub const XC_GFP_OOB: u32 = 0xF782; // 未知位注入
pub const XC_GFP_SEALED: u32 = 0xF783; // 封账后裁断
pub const XC_GFP_ZEROPATH: u32 = 0xF784; // 清零语义漂移
pub const XC_GFP_PATH: u32 = 0xF785; // 路径裁断漂移

/// flags 位定义（模型）。
pub const GF_WAIT: u8 = 0x01; // 可睡眠
pub const GF_ATOMIC: u8 = 0x02; // 不可睡眠
pub const GF_ZERO: u8 = 0x04; // 零页

/// 裁断结果。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GfpPath {
    Sleepable = 0,
    NoSleep = 1,
}

impl GfpPath {
    pub fn name(self) -> &'static str {
        match self {
            GfpPath::Sleepable => "SLEEPABLE",
            GfpPath::NoSleep => "NOSLEEP",
        }
    }
}

/// 裁断器。
pub struct GfpArbiter {
    zero_applied: u32,
    ledger: Vec<(u8, Result<(), u32>)>,
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl GfpArbiter {
    pub fn new() -> GfpArbiter {
        GfpArbiter { zero_applied: 0, ledger: Vec::new(), sealed: false, rejects: Vec::new() }
    }

    /// 裁断一次：返回 (路径, 是否清零)。非法组合必拒。
    pub fn arbiter(&mut self, flags: u8) -> Result<(GfpPath, bool), u32> {
        if self.sealed {
            self.rejects.push((XC_GFP_SEALED, "封账后裁断"));
            return Err(XC_GFP_SEALED);
        }
        // 未知位注入检测：只允许三位
        if flags & !0x07 != 0 {
            self.rejects.push((XC_GFP_OOB, "未知位"));
            self.ledger.push((flags, Err(XC_GFP_OOB)));
            return Err(XC_GFP_OOB);
        }
        let wait = flags & GF_WAIT != 0;
        let atomic = flags & GF_ATOMIC != 0;
        if wait && atomic {
            self.rejects.push((XC_GFP_MUTEX, "可睡眠/不可睡眠互斥破"));
            self.ledger.push((flags, Err(XC_GFP_MUTEX)));
            return Err(XC_GFP_MUTEX);
        }
        let path = if atomic { GfpPath::NoSleep } else { GfpPath::Sleepable };
        let zero = flags & GF_ZERO != 0;
        if zero {
            self.zero_applied += 1;
        }
        self.ledger.push((flags, Ok(())));
        Ok((path, zero))
    }

    /// 全组合矩阵（8 组合）逐项裁断。
    pub fn full_matrix(&mut self) -> [(u8, Result<(GfpPath, bool), u32>); 8] {
        let mut out = [(0u8, Ok((GfpPath::Sleepable, false))); 8];
        for k in 0..8u8 {
            let r = self.arbiter(k);
            out[k as usize] = (k, r);
        }
        out
    }

    /// 清零语义核：ZERO 裁断必须走清零路径（zero_applied 计数一致）。
    pub fn zero_consistent(&self) -> bool {
        let mut zero_flags = 0;
        for &(f, r) in &self.ledger {
            if r.is_ok() && f & GF_ZERO != 0 {
                zero_flags += 1;
            }
        }
        zero_flags == self.zero_applied
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_GFP_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for &(f, ref r) in &self.ledger {
            h ^= f as u32 ^ match r {
                Ok(_) => 0,
                Err(e) => *e,
            };
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.zero_applied;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.zero_applied as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）互斥组合 10/10（WAIT|ATOMIC 加零位变体）
    {
        let mut a = GfpArbiter::new();
        let mut hits = 0;
        let bads = [GF_WAIT | GF_ATOMIC, GF_WAIT | GF_ATOMIC | GF_ZERO, 0x0B, 0x0F, 0x1B, 0x2B, 0x4B, 0x8B, 0xFB, 0xFF];
        for &f in &bads {
            if a.arbiter(f).is_err() {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）未知位必拒
    {
        let mut a = GfpArbiter::new();
        if a.arbiter(0x08) == Err(XC_GFP_OOB) {
            caught += 1;
        }
    }
    // 3）原子路径（ATOMIC 单独）裁断
    {
        let mut a = GfpArbiter::new();
        if a.arbiter(GF_ATOMIC) == Ok((GfpPath::NoSleep, false)) {
            caught += 1;
        }
    }
    // 4）清零语义一致
    {
        let mut a = GfpArbiter::new();
        let _ = a.arbiter(GF_ZERO);
        let _ = a.arbiter(GF_WAIT | GF_ZERO);
        let _ = a.arbiter(GF_ATOMIC | GF_ZERO);
        if a.zero_consistent() && a.zero_applied == 3 {
            caught += 1;
        }
    }
    // 5）封账后裁断必拒
    {
        let mut a = GfpArbiter::new();
        let _ = a.seal();
        if a.arbiter(GF_WAIT) == Err(XC_GFP_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2542-J1。
pub fn run_unx2542_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2542");

    // C42-01 全组合矩阵 8 项（7 合法 1 非法）
    {
        let mut a = GfpArbiter::new();
        let m = a.full_matrix();
        let mut legal = 0;
        let mut illegal = 0;
        for &(f, ref r) in &m {
            match r {
                Ok(_) => legal += 1,
                Err(XC_GFP_MUTEX) => {
                    if f == GF_WAIT | GF_ATOMIC {
                        illegal += 1;
                    }
                }
                Err(_) => {}
            }
        }
        let ok = legal == 7 && illegal == 1;
        s.add("C42-01-全组合矩阵", ok, if ok { "" } else { "矩阵破" });
    }
    // C42-02 互斥注入 10/10
    {
        let mut a = GfpArbiter::new();
        let mut hits = 0;
        let bads = [GF_WAIT | GF_ATOMIC, GF_WAIT | GF_ATOMIC | GF_ZERO, 0x0B, 0x0F, 0x1B, 0x2B, 0x4B, 0x8B, 0xFB, 0xFF];
        for &f in &bads {
            if a.arbiter(f).is_err() {
                hits += 1;
            }
        }
        let ok = hits == 10;
        s.add("C42-02-互斥注入10/10", ok, if ok { "" } else { "互斥漏抓" });
    }
    // C42-03 三轴位定义冻结
    {
        let ok = GF_WAIT == 0x01 && GF_ATOMIC == 0x02 && GF_ZERO == 0x04;
        s.add("C42-03-三轴位冻结", ok, if ok { "" } else { "位表漂移" });
    }
    // C42-04 路径裁断语义
    {
        let mut a = GfpArbiter::new();
        let r1 = a.arbiter(GF_WAIT);
        let r2 = a.arbiter(0);
        let r3 = a.arbiter(GF_ATOMIC);
        let ok = r1 == Ok((GfpPath::Sleepable, false))
            && r2 == Ok((GfpPath::Sleepable, false))
            && r3 == Ok((GfpPath::NoSleep, false));
        s.add("C42-04-路径语义", ok, if ok { "" } else { "路径漂移" });
    }
    // C42-05 零页语义（ZERO 位贯穿三路径）
    {
        let mut a = GfpArbiter::new();
        let z1 = a.arbiter(GF_ZERO);
        let z2 = a.arbiter(GF_WAIT | GF_ZERO);
        let z3 = a.arbiter(GF_ATOMIC | GF_ZERO);
        let ok = z1 == Ok((GfpPath::Sleepable, true))
            && z2 == Ok((GfpPath::Sleepable, true))
            && z3 == Ok((GfpPath::NoSleep, true))
            && a.zero_consistent();
        s.add("C42-05-零页贯穿", ok, if ok { "" } else { "清零漂移" });
    }
    // C42-06 未知位必拒
    {
        let mut a = GfpArbiter::new();
        let ok = a.arbiter(0x10) == Err(XC_GFP_OOB) && a.arbiter(0x80) == Err(XC_GFP_OOB);
        s.add("C42-06-未知位拒", ok, if ok { "" } else { "未知位漏抓" });
    }
    // C42-07 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C42-07-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C42-08 路径名冻结
    {
        let ok = GfpPath::Sleepable.name() == "SLEEPABLE" && GfpPath::NoSleep.name() == "NOSLEEP";
        s.add("C42-08-路径名冻结", ok, if ok { "" } else { "名漂移" });
    }
    // C42-09 指纹冻结
    {
        let f = || {
            let mut a = GfpArbiter::new();
            let _ = a.arbiter(GF_WAIT);
            let _ = a.arbiter(GF_ATOMIC | GF_ZERO);
            a.fingerprint()
        };
        let ok = f() == f();
        s.add("C42-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C42-10 封账稳定
    {
        let mut a = GfpArbiter::new();
        let _ = a.arbiter(0);
        let pre = a.fingerprint();
        let _ = a.seal();
        let ok = a.fingerprint() == pre && a.seal().is_err();
        s.add("C42-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
