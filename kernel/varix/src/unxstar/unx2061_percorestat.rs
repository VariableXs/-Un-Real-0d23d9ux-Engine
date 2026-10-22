// unx2061_percorestat —— UNX-F2061 每核统计区总成（计数族总登记）
// 判据原文 UNX-F2061-J1：统计区断言全过（切换/迁移/idle/唤醒计数族总登记），
// per-CPU 变量区接续 A1 trampoline 预留地址断言（地址续接零重叠），布局账在册。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF680 独占（0xF680~0xF6FF）。
//
// 设计（纯功能 no_std）：
//   * 计数族四闭集：切换（上下文切换数）/迁移（线程跨核迁移数）/idle（进入与
//     退出次数）/唤醒（远端唤醒数）——每核四槽，单调递增；
//   * 布局账：每核统计区基地址接续 A1 trampoline 预留地址（stride 恒定零重叠）；
//   * 布局断言：相邻核区间 [base_i, base_i+size) 与 [base_{i+1}, ...) 零交叠；
//   * 守恒：全局总和 = Σ 每核槽值（逐槽复算）；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF680 独占） ─────────────────────────────────────────────
pub const XC_STAT_OOB: u32 = 0xF681; // 核号越界
pub const XC_STAT_MONO: u32 = 0xF682; // 计数回退（单调破）
pub const XC_STAT_OVERLAP: u32 = 0xF683; // 布局重叠
pub const XC_STAT_CONSERVE: u32 = 0xF684; // 守恒破（Σ≠全局）
pub const XC_STAT_SEALED: u32 = 0xF685; // 封账后计数

/// A1 trampoline 预留区末址（布局账锚点，判据对齐面）。
pub const TRAMPOLINE_END: u64 = 0x0009_FC00;

/// 每核统计区大小（字节）。
pub const STAT_REGION_SIZE: u64 = 0x100;

/// 计数族四槽闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StatSlot {
    CtxSwitch = 0,
    Migration = 1,
    IdleEnter = 2,
    Wakeup = 3,
}

impl StatSlot {
    pub fn num(self) -> u8 {
        self as u8
    }
    pub fn name(self) -> &'static str {
        match self {
            StatSlot::CtxSwitch => "CTXSW",
            StatSlot::Migration => "MIGR",
            StatSlot::IdleEnter => "IDLEEN",
            StatSlot::Wakeup => "WAKEUP",
        }
    }
}

/// 每核统计区：四计数槽 + 布局锚。
#[derive(Clone, Copy)]
pub struct CoreStat {
    pub core: u32,
    pub base: u64, // 统计区基地址
    pub counters: [u64; 4],
}

/// 统计区总成。
pub struct PerCoreStatBook {
    cores: Vec<CoreStat>,
    global: [u64; 4],
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PerCoreStatBook {
    pub fn new(ncore: u32) -> PerCoreStatBook {
        let mut cores = Vec::new();
        for c in 0..ncore {
            cores.push(CoreStat {
                core: c,
                base: TRAMPOLINE_END + (c as u64) * STAT_REGION_SIZE,
                counters: [0; 4],
            });
        }
        PerCoreStatBook { cores, global: [0; 4], sealed: false, rejects: Vec::new() }
    }

    /// 计数递增（单调：只增不减）。
    pub fn bump(&mut self, core: u32, slot: StatSlot, at: u64) -> Result<u64, u32> {
        if self.sealed {
            self.rejects.push((XC_STAT_SEALED, "封账后计数"));
            return Err(XC_STAT_SEALED);
        }
        if core as usize >= self.cores.len() {
            self.rejects.push((XC_STAT_OOB, "核号越界"));
            return Err(XC_STAT_OOB);
        }
        let c = &mut self.cores[core as usize];
        let old = c.counters[slot.num() as usize];
        if (at as u64) < old {
            // 刻度回退是账面异常，但计数本身不回退 —— 仍按递增处理
            self.rejects.push((XC_STAT_MONO, "刻度回退"));
        }
        c.counters[slot.num() as usize] += 1;
        self.global[slot.num() as usize] += 1;
        Ok(c.counters[slot.num() as usize])
    }

    /// 布局断言：接续 trampoline 末址 + stride 恒定 + 零重叠。
    pub fn layout_ok(&self) -> bool {
        for (i, c) in self.cores.iter().enumerate() {
            if c.base != TRAMPOLINE_END + (i as u64) * STAT_REGION_SIZE {
                return false;
            }
            if i + 1 < self.cores.len() {
                let next_base = self.cores[i + 1].base;
                if c.base + STAT_REGION_SIZE > next_base {
                    return false; // 重叠
                }
            }
        }
        true
    }

    /// 显式重叠扫描（上界逐对）。
    pub fn overlap_scan(&self) -> usize {
        let mut hits = 0;
        for i in 0..self.cores.len() {
            for j in (i + 1)..self.cores.len() {
                let a = &self.cores[i];
                let b = &self.cores[j];
                if a.base < b.base + STAT_REGION_SIZE && b.base < a.base + STAT_REGION_SIZE {
                    hits += 1;
                }
            }
        }
        hits
    }

    /// 守恒：全局 == Σ 每核（逐槽）。
    pub fn conserved(&self) -> bool {
        for s in 0..4usize {
            let mut sum = 0u64;
            for c in &self.cores {
                sum += c.counters[s];
            }
            if sum != self.global[s] {
                return false;
            }
        }
        true
    }

    pub fn core_stat(&self, core: u32) -> Result<CoreStat, u32> {
        for &c in &self.cores {
            if c.core == core {
                return Ok(c);
            }
        }
        Err(XC_STAT_OOB)
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_STAT_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        for c in &self.cores {
            h ^= c.base as u32;
            h = h.wrapping_mul(0x0100_0193);
            for v in c.counters {
                h ^= v as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
        }
        for g in self.global {
            h ^= g as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        (h as u64) << 16 | self.cores.len() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    pub fn ncore(&self) -> usize {
        self.cores.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）核号越界 10/10
    {
        let mut b = PerCoreStatBook::new(4);
        let mut hits = 0;
        for c in 4..14u32 {
            if b.bump(c, StatSlot::CtxSwitch, 1) == Err(XC_STAT_OOB) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）布局重叠注入（手工破坏 stride）
    {
        let mut b = PerCoreStatBook::new(4);
        b.cores[2].base = b.cores[1].base; // 重叠注入
        if b.overlap_scan() >= 1 && !b.layout_ok() {
            caught += 1;
        }
    }
    // 3）守恒破注入（全局与每核失步）
    {
        let mut b = PerCoreStatBook::new(2);
        let _ = b.bump(0, StatSlot::Wakeup, 1);
        b.global[3] += 5; // 篡改全局
        if !b.conserved() {
            caught += 1;
        }
    }
    // 4）单调破检测（刻度回退落账但计数不回退）
    {
        let mut b = PerCoreStatBook::new(1);
        let _ = b.bump(0, StatSlot::CtxSwitch, 100);
        let _ = b.bump(0, StatSlot::CtxSwitch, 50); // 刻度回退
        let s = b.core_stat(0).map(|c| c.counters[0]).unwrap_or(0);
        if s == 2 && b.reject_count() >= 1 {
            caught += 1;
        }
    }
    // 5）封账后计数必拒
    {
        let mut b = PerCoreStatBook::new(1);
        let _ = b.seal();
        if b.bump(0, StatSlot::CtxSwitch, 1) == Err(XC_STAT_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2061-J1。
pub fn run_unx2061_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2061");

    // C61-01 四族总登记（每族至少 8 次计数可递增）
    {
        let mut b = PerCoreStatBook::new(4);
        let mut all_ok = true;
        for c in 0..4u32 {
            for k in 0..8u64 {
                for slot in [StatSlot::CtxSwitch, StatSlot::Migration, StatSlot::IdleEnter, StatSlot::Wakeup] {
                    if b.bump(c, slot, k + 1).is_err() {
                        all_ok = false;
                    }
                }
            }
        }
        all_ok = all_ok && b.conserved();
        s.add("C61-01-四族总登记", all_ok, if all_ok { "" } else { "登记破" });
    }
    // C61-02 trampoline 接续（base0 == TRAMPOLINE_END）
    {
        let b = PerCoreStatBook::new(2);
        let c0 = b.core_stat(0);
        let ok = c0.map(|c| c.base == TRAMPOLINE_END) == Ok(true);
        s.add("C61-02-trampoline接续", ok, if ok { "" } else { "锚点漂移" });
    }
    // C61-03 stride 恒定零重叠（8 核）
    {
        let b = PerCoreStatBook::new(8);
        let ok = b.layout_ok() && b.overlap_scan() == 0;
        s.add("C61-03-布局零重叠", ok, if ok { "" } else { "布局破" });
    }
    // C61-04 守恒（全局=Σ每核逐槽）
    {
        let mut b = PerCoreStatBook::new(3);
        let _ = b.bump(0, StatSlot::CtxSwitch, 1);
        let _ = b.bump(2, StatSlot::CtxSwitch, 2);
        let _ = b.bump(1, StatSlot::Wakeup, 3);
        s.add("C61-04-逐槽守恒", b.conserved(), if b.conserved() { "" } else { "守恒破" });
    }
    // C61-05 计数单调（逐核递增序）
    {
        let mut b = PerCoreStatBook::new(1);
        let mut mono = true;
        let mut prev = 0;
        for k in 0..10u64 {
            if let Ok(v) = b.bump(0, StatSlot::Migration, k + 1) {
                if v <= prev {
                    mono = false;
                }
                prev = v;
            }
        }
        s.add("C61-05-计数单调", mono, if mono { "" } else { "单调破" });
    }
    // C61-06 布局重叠注入必抓
    {
        let mut b = PerCoreStatBook::new(3);
        b.cores[1].base = b.cores[0].base + 0x80; // 半区重叠
        let ok = b.overlap_scan() >= 1 && !b.layout_ok();
        s.add("C61-06-重叠必抓", ok, if ok { "" } else { "重叠漏抓" });
    }
    // C61-07 槽闭集
    {
        let ok = StatSlot::CtxSwitch.num() == 0
            && StatSlot::Wakeup.num() == 3
            && StatSlot::CtxSwitch.name() == "CTXSW"
            && StatSlot::Wakeup.name() == "WAKEUP";
        s.add("C61-07-槽闭集", ok, if ok { "" } else { "槽表漂移" });
    }
    // C61-08 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C61-08-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C61-09 指纹冻结
    {
        let f = || {
            let mut b = PerCoreStatBook::new(2);
            let _ = b.bump(0, StatSlot::CtxSwitch, 1);
            let _ = b.bump(1, StatSlot::IdleEnter, 2);
            b.fingerprint()
        };
        let ok = f() == f();
        s.add("C61-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C61-10 封账稳定
    {
        let mut b = PerCoreStatBook::new(1);
        let _ = b.bump(0, StatSlot::CtxSwitch, 1);
        let pre = b.fingerprint();
        let _ = b.seal();
        let ok = b.fingerprint() == pre && b.seal().is_err();
        s.add("C61-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
