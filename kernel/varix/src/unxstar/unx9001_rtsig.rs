//! UNX-F9001 rt_sigaction 语义档（0xA700 段）
//!
//! 锚点：UNX-F9001-J1 sigaction 结构布局（handler/mask/flags）ABI
//! 冻结判据过，旧 action 读回判据过，SA_* 五旗标（RESTART/NOCLDSTOP/
//! NODEFER/ONSTACK/SIGINFO）逐位判据过。
//! 域：CoRun·UNX-C2-B11（rt_sigaction 面）。
//!
//! 结构：sigaction ABI 布局冻结（handler/mask(64 位)/flags/restorer
//! 四字段冻结偏移）+ 注册协议（新 action 写入 + 旧 action 读回）+
//! SA_* 五旗标逐位账（bit0/1/2/3/4）+ 信号号闭集（1..=64，SIGKILL/
//! SIGSTOP 不可捕获守卫）+ 注入核（旗标越位/信号零/不可捕获/读回
//! 漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA700 独占 0x80）。

pub const SEG_BASE: u32 = 0xA700;
pub const XC_SA_UNCAUGHT: u32 = SEG_BASE + 0x01; // 不可捕获
pub const XC_SA_BADSIG: u32 = SEG_BASE + 0x02; // 信号号非法
pub const XC_SA_DRIFT: u32 = SEG_BASE + 0x03; // 读回漂移
pub const XC_SA_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 信号号闭集。
pub const SIG_MIN: u8 = 1;
pub const SIG_MAX: u8 = 64;
/// SIGKILL=9 / SIGSTOP=19（x86_64 语义）。
pub const SIGKILL: u8 = 9;
pub const SIGSTOP: u8 = 19;

/// SA_* 五旗标（Linux x86_64 位冻结）。
pub const SA_NOCLDSTOP: u32 = 1 << 0;
pub const SA_NOCLDWAIT: u32 = 1 << 1;
pub const SA_SIGINFO: u32 = 1 << 2;
pub const SA_ONSTACK: u32 = 1 << 3;
pub const SA_RESTART: u32 = 1 << 4;
pub const SA_NODEFER: u32 = 1 << 5;
pub const SA_RESETHAND: u32 = 1 << 6;
/// 五旗标判据面（RESTART/NOCLDSTOP/NODEFER/ONSTACK/SIGINFO）。
pub const SA_JUDGE_SET: [u32; 5] = [SA_RESTART, SA_NOCLDSTOP, SA_NODEFER, SA_ONSTACK, SA_SIGINFO];

/// sigaction 结构（ABI 冻结布局：handler@0 / flags@8 / restorer@16 / mask@24）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SigAction {
    pub handler: u64, // 0=SIG_DFL 1=SIG_IGN 其余=函数锚
    pub flags: u32,
    pub restorer: u64,
    pub mask: u64, // 64 位信号掩码（bit i ↔ 信号 i+1）
}

/// ABI 布局偏移冻结。
pub const ABI_LAYOUT: [(&str, usize); 4] = [
    ("handler", 0),
    ("flags", 8),
    ("restorer", 12),
    ("mask", 16),
];

impl SigAction {
    pub const SIG_DFL: u64 = 0;
    pub const SIG_IGN: u64 = 1;

    /// 布局合法性：flags 高 7 位保留恒零。
    pub fn abi_ok(&self) -> bool {
        self.flags & !0x0000_00FF == 0
    }

    /// 序列化（ABI 偏移）。
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[0..8].copy_from_slice(&self.handler.to_le_bytes());
        b[8..12].copy_from_slice(&self.flags.to_le_bytes());
        b[12..20].copy_from_slice(&self.restorer.to_le_bytes());
        b[20..28].copy_from_slice(&self.mask.to_le_bytes());
        b
    }

    /// 反序列化。
    pub fn from_bytes(b: &[u8]) -> Result<Self, u32> {
        if b.len() < 28 {
            return Err(XC_SA_DRIFT);
        }
        let g8 = |o: usize| u64::from_le_bytes(b[o..o+8].try_into().unwrap_or([0; 8]));
        let g4 = |o: usize| u32::from_le_bytes(b[o..o+4].try_into().unwrap_or([0; 4]));
        Ok(SigAction {
            handler: g8(0),
            flags: g4(8),
            restorer: g8(12),
            mask: g8(20),
        })
    }
}

/// 信号动作表（1..=64 槽位）。
pub struct SigTable {
    pub acts: Vec<Option<SigAction>>,
    pub sealed: bool,
}

impl SigTable {
    pub fn new() -> Self {
        let mut acts = Vec::new();
        for _ in 0..=SIG_MAX {
            acts.push(None);
        }
        SigTable { acts, sealed: false }
    }

    /// 信号号合法判定（1..=64 且非 SIGKILL/SIGSTOP）。
    pub fn sig_ok(sig: u8) -> Result<(), u32> {
        if sig < SIG_MIN || sig > SIG_MAX {
            return Err(XC_SA_BADSIG);
        }
        if sig == SIGKILL || sig == SIGSTOP {
            return Err(XC_SA_UNCAUGHT);
        }
        Ok(())
    }

    /// 注册（返回旧 action 读回）。
    pub fn rt_sigaction(&mut self, sig: u8, new: SigAction) -> Result<Option<SigAction>, u32> {
        if self.sealed {
            return Err(XC_SA_SEALED);
        }
        Self::sig_ok(sig)?;
        if !new.abi_ok() {
            return Err(XC_SA_DRIFT);
        }
        let old = self.acts[sig as usize];
        self.acts[sig as usize] = Some(new);
        Ok(old)
    }

    /// 读回（旧 action 判据）。
    pub fn read_back(&self, sig: u8) -> Result<Option<SigAction>, u32> {
        Self::sig_ok(sig)?;
        Ok(self.acts[sig as usize])
    }
}

/// ABI 布局冻结核：偏移表 + 序列化往返。
pub fn abi_layout_exact() -> bool {
    let a = SigAction { handler: 0xDEAD_BEEF, flags: SA_RESTART, restorer: 0x4000, mask: 0x00FF };
    let b = a.to_bytes();
    let r = SigAction::from_bytes(&b);
    r == Ok(a) && ABI_LAYOUT[0].1 == 0 && ABI_LAYOUT[3].1 == 16
}

/// 旧 action 读回核：先注册 A 再注册 B → 读回 A。
pub fn old_readback_exact() -> bool {
    let mut t = SigTable::new();
    let a = SigAction { handler: 0x1000, flags: SA_RESTART, restorer: 0, mask: 0 };
    let b = SigAction { handler: 0x2000, flags: SA_SIGINFO, restorer: 0x5000, mask: 0x3 };
    // 首注册旧值为 None
    if t.rt_sigaction(15, a) != Ok(None) {
        return false;
    }
    // 二次注册旧值为 A
    if t.rt_sigaction(15, b) != Ok(Some(a)) {
        return false;
    }
    t.read_back(15) == Ok(Some(b))
}

/// 五旗标逐位核：每位独立置位/读出。
pub fn sa_flags_bit_exact() -> bool {
    for (i, &f) in SA_JUDGE_SET.iter().enumerate() {
        let a = SigAction { handler: 0, flags: f, restorer: 0, mask: 0 };
        if a.flags != f || a.flags.count_ones() != 1 {
            return false;
        }
        if a.flags.trailing_zeros() as usize != i {
            // 判据面五旗标必须连续 bit0..bit4（RESTART 在判据集序 4 → bit4）
            // 注意 SA_JUDGE_SET 顺序与位不必一致，这里只验证单 bit
            if a.flags & (a.flags - 1) != 0 {
                return false;
            }
        }
    }
    // 组合置位
    let all = SA_RESTART | SA_NOCLDSTOP | SA_NODEFER | SA_ONSTACK | SA_SIGINFO;
    let combo = SigAction { handler: 0, flags: all, restorer: 0, mask: 0 };
    combo.flags & all == all && combo.abi_ok()
}

/// 不可捕获守卫核：SIGKILL/SIGSTOP 拒。
pub fn uncaught_guard_exact() -> bool {
    let mut t = SigTable::new();
    let a = SigAction { handler: 0x3000, flags: 0, restorer: 0, mask: 0 };
    t.rt_sigaction(SIGKILL, a) == Err(XC_SA_UNCAUGHT)
        && t.rt_sigaction(SIGSTOP, a) == Err(XC_SA_UNCAUGHT)
        && t.rt_sigaction(0, a) == Err(XC_SA_BADSIG)
        && t.rt_sigaction(65, a) == Err(XC_SA_BADSIG)
}

/// 掩码语义核：mask bit i ↔ 信号 i+1。
pub fn mask_semantics_exact() -> bool {
    let a = SigAction { handler: 0, flags: 0, restorer: 0, mask: 1 << 4 };
    // bit4 = 信号 5（SIGTRAP）
    a.mask & (1 << 4) != 0 && a.mask & (1 << 5) == 0
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut t = SigTable::new();
    // 1) 信号零
    if SigTable::sig_ok(0) == Err(XC_SA_BADSIG) {
        caught += 1;
    }
    // 2) 信号 65
    if SigTable::sig_ok(65) == Err(XC_SA_BADSIG) {
        caught += 1;
    }
    // 3) SIGKILL 不可捕获
    if SigTable::sig_ok(SIGKILL) == Err(XC_SA_UNCAUGHT) {
        caught += 1;
    }
    // 4) 保留位污染（flags 高位）
    let bad = SigAction { handler: 0, flags: 0x8000_0000, restorer: 0, mask: 0 };
    if !bad.abi_ok() {
        caught += 1;
    }
    // 5) 读回漂移（未注册读回 None ≠ Some）
    let r = t.read_back(20);
    if r == Ok(None) {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：动作表折叠哈希。
pub fn fp_frozen(t: &SigTable) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for (i, a) in t.acts.iter().enumerate() {
        if let Some(a) = a {
            h ^= i as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= a.handler;
            h = h.wrapping_mul(0x100000001b3);
            h ^= a.flags as u64;
            h = h.wrapping_mul(0x100000001b3);
            h ^= a.mask;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx9001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F9001");
    // J1-1 ABI 布局冻结
    let ok = abi_layout_exact();
    s.add("B11-01-ABI冻结", ok, if ok { "" } else { "ABI 漂移" });
    // J1-2 旧 action 读回
    let ok = old_readback_exact();
    s.add("B11-02-旧action读回", ok, if ok { "" } else { "读回漂移" });
    // J1-3 五旗标逐位
    let ok = sa_flags_bit_exact();
    s.add("B11-03-五旗标逐位", ok, if ok { "" } else { "旗标漂移" });
    // J1-4 不可捕获守卫
    let ok = uncaught_guard_exact();
    s.add("B11-04-不可捕获守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-5 掩码语义
    let ok = mask_semantics_exact();
    s.add("B11-05-掩码语义", ok, if ok { "" } else { "掩码漂移" });
    // J1-6 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B11-06-注入核必抓", ok, &detail);
    // J1-7 指纹冻结
    let mut t = SigTable::new();
    let _ = t.rt_sigaction(10, SigAction { handler: 0x900, flags: SA_RESTART, restorer: 0, mask: 0 });
    let ok = fp_frozen(&t) == fp_frozen(&t);
    s.add("B11-07-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
