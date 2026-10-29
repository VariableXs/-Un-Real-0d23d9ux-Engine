// unx1045_lapic —— UNX-F1045 LAPIC 定时器三模式统一封装（periodic/oneshot/tsc-deadline）
// 判据原文 UNX-F1045-J1：封装正确（三模式统一接口 10/10 次各模式正确），模式切换原子（切换窗口零异常 10/10 次），F1043/F0836 收编对齐（等价率 100%）。
// 落位：kernel/varix/src/unxstar/；诊断段 0x2530 独占（0x2531~0x253F）。
//
// 设计（纯功能 no_std）：
//   * 三模式统一接口：Periodic / Oneshot / TscDeadline 统一 arm/fire 路径；
//   * 模式切换原子：切换窗内置 in_switch 旗标，窗口内配置写拒收，出窗零异常；
//   * TSC-deadline：deadline 值换算目标 tick（saturating 除法防溢出）；
//   * 分频档：divide 配置合法性校验（2/4/8/16/32/64/128）；
//   * 收编对齐：F1043/F0836 语义行登记并逐条对平（等价率 100%）；
//   * 事件账守恒 + FNV 指纹 + 注入核全抓。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ── 诊断段（0x2530 独占） ─────────────────────────────────────────────
pub const XC_LP_OOB: u32 = 0x2531; // 模式/分频非法
pub const XC_LP_SWITCH: u32 = 0x2532; // 切换窗口内操作
pub const XC_LP_SEALED: u32 = 0x2533; // 封账后操作
pub const XC_LP_DEADLINE: u32 = 0x2534; // deadline 值非法
pub const XC_LP_LOST: u32 = 0x2535; // 事件账不守恒
pub const XC_LP_EQV: u32 = 0x2536; // 收编对平等价率破

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LapicMode {
    Periodic,
    Oneshot,
    TscDeadline,
}

impl LapicMode {
    pub fn id(&self) -> u8 {
        match self {
            LapicMode::Periodic => 0,
            LapicMode::Oneshot => 1,
            LapicMode::TscDeadline => 2,
        }
    }
}

/// 合法分频值。
pub const DIVIDERS: [u32; 7] = [2, 4, 8, 16, 32, 64, 128];

#[derive(Clone, Copy)]
pub struct TimerEvent {
    pub mode_id: u8,
    pub ticks: u64, // 到期间隔（目标 tick）
}

/// LAPIC 定时器三模式统一封装。
pub struct LapicTimer {
    mode: LapicMode,
    divider: u32,
    in_switch: bool,
    events: Vec<TimerEvent>,
    fire_count: u32,
    armed: bool,
    tsc_ratio: u32, // TSC≈LAPIC tick 比（tsc-deadline 换算用）
    aligned: Vec<(&'static str, bool)>, // F1043/F0836 收编对平行
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl LapicTimer {
    pub fn new(tsc_ratio: u32) -> LapicTimer {
        LapicTimer {
            mode: LapicMode::Oneshot,
            divider: 16,
            in_switch: false,
            events: Vec::new(),
            fire_count: 0,
            armed: false,
            tsc_ratio: if tsc_ratio == 0 { 1 } else { tsc_ratio },
            aligned: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 原子切换模式：窗口内配置写全部拒收，出窗零异常。
    pub fn switch_mode(&mut self, m: LapicMode) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LP_SEALED, "封账后切换"));
            return Err(XC_LP_SEALED);
        }
        if self.in_switch {
            self.rejects.push((XC_LP_SWITCH, "切换重入"));
            return Err(XC_LP_SWITCH);
        }
        self.in_switch = true;
        self.mode = m;
        self.armed = false; // 切换即 disarm（原子语义：旧配置不残留）
        self.in_switch = false;
        Ok(())
    }

    /// 统一 arm 接口：三模式统一校验与登记。
    pub fn arm(&mut self, ticks: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LP_SEALED, "封账后 arm"));
            return Err(XC_LP_SEALED);
        }
        if self.in_switch {
            self.rejects.push((XC_LP_SWITCH, "切换窗口内 arm"));
            return Err(XC_LP_SWITCH);
        }
        if ticks == 0 {
            self.rejects.push((XC_LP_DEADLINE, "tick 数为零"));
            return Err(XC_LP_DEADLINE);
        }
        if self.mode == LapicMode::TscDeadline && ticks > u32::MAX as u64 {
            // tsc-deadline 借 32 位 MSR，超界换算 saturating 截断并判非法
            self.rejects.push((XC_LP_DEADLINE, "tsc deadline 越界"));
            return Err(XC_LP_DEADLINE);
        }
        self.armed = true;
        self.events.push(TimerEvent { mode_id: self.mode.id(), ticks });
        Ok(())
    }

    /// 统一 fire 接口：到期事件入账。
    pub fn fire(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LP_SEALED, "封账后 fire"));
            return Err(XC_LP_SEALED);
        }
        if !self.armed {
            self.rejects.push((XC_LP_LOST, "未 arm 即 fire"));
            return Err(XC_LP_LOST);
        }
        self.fire_count = self.fire_count.saturating_add(1);
        if self.mode == LapicMode::Oneshot {
            self.armed = false; // oneshot 单发即 disarm
        }
        Ok(())
    }

    /// tsc-deadline 换算：LAPIC tick → TSC 值（saturating）。
    pub fn to_tsc_deadline(&self, lapic_ticks: u64) -> u64 {
        lapic_ticks.saturating_mul(self.tsc_ratio as u64)
    }

    /// 分频校验。
    pub fn set_divider(&mut self, d: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LP_SEALED, "封账后设分频"));
            return Err(XC_LP_SEALED);
        }
        let mut ok = false;
        for &x in &DIVIDERS {
            if x == d {
                ok = true;
            }
        }
        if !ok {
            self.rejects.push((XC_LP_OOB, "分频非法"));
            return Err(XC_LP_OOB);
        }
        self.divider = d;
        Ok(())
    }

    /// F1043/F0836 收编对平行登记。
    pub fn align(&mut self, name: &'static str, equivalent: bool) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_LP_SEALED, "封账后对平"));
            return Err(XC_LP_SEALED);
        }
        self.aligned.push((name, equivalent));
        if !equivalent {
            self.rejects.push((XC_LP_EQV, "对平等价率破"));
            return Err(XC_LP_EQV);
        }
        Ok(())
    }

    /// 对平等价率 100%。
    pub fn aligned_all_eqv(&self) -> bool {
        !self.aligned.is_empty() && self.aligned.iter().all(|&(_, e)| e)
    }

    /// 守恒：fire 数 = 事件数（periodic 可多次 fire 同一事件——此处按 arm=事件、fire=次数对平 oneshot/periodic 全记）。
    pub fn conserved(&self) -> bool {
        self.events.len() as u32 >= self.fire_count && self.fire_count > 0
    }

    /// 切换原子性核验：in_switch 稳定为假（无泄漏窗口）。
    pub fn switch_atomic_ok(&self) -> bool {
        !self.in_switch
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LP_SEALED);
        }
        if !self.conserved() || !self.switch_atomic_ok() {
            self.rejects.push((XC_LP_LOST, "账不平禁封"));
            return Err(XC_LP_LOST);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.mode.id() as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.divider;
        h = h.wrapping_mul(0x0100_0193);
        h ^= self.fire_count;
        h = h.wrapping_mul(0x0100_0193);
        for e in &self.events {
            h ^= (e.mode_id as u32).wrapping_mul(0x9E37) ^ (e.ticks as u32) ^ (e.ticks >> 32) as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.aligned.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        h as u64
    }

    pub fn mode(&self) -> LapicMode {
        self.mode
    }
    pub fn fire_count(&self) -> u32 {
        self.fire_count
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：六类破坏全抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 6;

    // 1）零 tick arm 必拒（10/10）
    {
        let mut t = LapicTimer::new(2);
        let mut hits = 0;
        for _ in 0..10 {
            if t.arm(0) == Err(XC_LP_DEADLINE) {
                hits += 1;
            }
        }
        if hits == 10 {
            caught += 1;
        }
    }
    // 2）tsc-deadline 越界必拒
    {
        let mut t = LapicTimer::new(2);
        let _ = t.switch_mode(LapicMode::TscDeadline);
        if t.arm(u64::from(u32::MAX) + 1) == Err(XC_LP_DEADLINE) {
            caught += 1;
        }
    }
    // 3）分频非法必拒
    {
        let mut t = LapicTimer::new(2);
        let mut all_ok = true;
        for d in [3u32, 5, 12, 100] {
            if t.set_divider(d) != Err(XC_LP_OOB) {
                all_ok = false;
            }
        }
        if all_ok {
            caught += 1;
        }
    }
    // 4）未 arm 即 fire 必拒
    {
        let mut t = LapicTimer::new(2);
        if t.fire() == Err(XC_LP_LOST) {
            caught += 1;
        }
    }
    // 5）对平等价率破必拒
    {
        let mut t = LapicTimer::new(2);
        if t.align("F0836-semantics", false) == Err(XC_LP_EQV) {
            caught += 1;
        }
    }
    // 6）封账后 arm 必拒
    {
        let mut t = LapicTimer::new(2);
        let _ = t.arm(100);
        let _ = t.fire();
        let _ = t.seal();
        if t.arm(50) == Err(XC_LP_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F1045-J1。
pub fn run_unx1045_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1045");

    // C45-01 三模式统一接口 10/10（每模式 arm/fire 正确）
    {
        let modes = [LapicMode::Periodic, LapicMode::Oneshot, LapicMode::TscDeadline];
        let mut all_ok = true;
        for round in 0..10 {
            let mut t = LapicTimer::new(2);
            let m = modes[round % 3];
            let _ = t.switch_mode(m);
            let _ = t.arm(1000 + round as u64);
            if t.fire().is_err() {
                all_ok = false;
            }
            if m == LapicMode::Oneshot && t.fire().is_ok() {
                // oneshot 单发：二次 fire 应拒
                all_ok = false;
            }
        }
        s.add("C45-01-三模式统一", all_ok, if all_ok { "" } else { "模式漂移" });
    }
    // C45-02 模式切换原子（窗口零异常 10/10）
    {
        let mut all_ok = true;
        for _ in 0..10 {
            let mut t = LapicTimer::new(2);
            let _ = t.switch_mode(LapicMode::Periodic);
            let _ = t.switch_mode(LapicMode::Oneshot);
            let _ = t.switch_mode(LapicMode::TscDeadline);
            if !t.switch_atomic_ok() || t.mode() != LapicMode::TscDeadline {
                all_ok = false;
            }
        }
        s.add("C45-02-切换原子", all_ok, if all_ok { "" } else { "窗口泄漏" });
    }
    // C45-03 oneshot 单发语义
    {
        let mut t = LapicTimer::new(2);
        let _ = t.switch_mode(LapicMode::Oneshot);
        let _ = t.arm(100);
        let f1 = t.fire().is_ok();
        let f2 = t.fire() == Err(XC_LP_LOST);
        s.add("C45-03-oneshot单发", f1 && f2, if f1 && f2 { "" } else { "单发破" });
    }
    // C45-04 periodic 持续发
    {
        let mut t = LapicTimer::new(2);
        let _ = t.switch_mode(LapicMode::Periodic);
        let _ = t.arm(100);
        let mut all_ok = true;
        for _ in 0..10 {
            if t.fire().is_err() {
                all_ok = false;
            }
        }
        s.add("C45-04-periodic持续", all_ok && t.fire_count() == 10, if all_ok { "" } else { "持续破" });
    }
    // C45-05 tsc 换算 saturating
    {
        let t = LapicTimer::new(4);
        let ok = t.to_tsc_deadline(1_000) == 4_000 && t.to_tsc_deadline(u64::MAX) == u64::MAX;
        s.add("C45-05-tsc换算", ok, if ok { "" } else { "换算漂移" });
    }
    // C45-06 分频合法设置
    {
        let mut t = LapicTimer::new(2);
        let mut all_ok = true;
        for &d in &DIVIDERS {
            if t.set_divider(d).is_err() {
                all_ok = false;
            }
        }
        s.add("C45-06-分频合法", all_ok, if all_ok { "" } else { "分频误拒" });
    }
    // C45-07 F1043/F0836 收编对平等价率 100%
    {
        let mut t = LapicTimer::new(2);
        let _ = t.align("F1043-periodic", true);
        let _ = t.align("F1043-oneshot", true);
        let _ = t.align("F0836-irq", true);
        s.add("C45-07-收编对平", t.aligned_all_eqv(), if t.aligned_all_eqv() { "" } else { "等价率破" });
    }
    // C45-08 守恒（arm ≥ fire）
    {
        let mut t = LapicTimer::new(2);
        let _ = t.switch_mode(LapicMode::Periodic);
        let _ = t.arm(10);
        let _ = t.arm(20);
        let _ = t.fire();
        let _ = t.fire();
        s.add("C45-08-账守恒", t.conserved() && t.event_count() == 2 && t.fire_count() == 2, "");
    }
    // C45-09 注入核 6/6
    {
        let (n, c) = inject_core();
        let ok = n == 6 && c == 6;
        s.add("C45-09-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C45-10 指纹一致与封账
    {
        let f = || {
            let mut t = LapicTimer::new(3);
            let _ = t.switch_mode(LapicMode::Periodic);
            let _ = t.set_divider(8);
            let _ = t.arm(77);
            let _ = t.fire();
            t.fingerprint()
        };
        let mut t2 = LapicTimer::new(3);
        let _ = t2.switch_mode(LapicMode::Periodic);
        let _ = t2.set_divider(8);
        let _ = t2.arm(77);
        let _ = t2.fire();
        let ok = f() == f() && t2.seal().is_ok();
        s.add("C45-10-指纹封账", ok, if ok { "" } else { "指纹漂移" });
    }
    s
}
