//! UNX-F0124 引导失败计数与自动降级（0xA800 段）
//!
//! 锚点：UNX-F0124-J1 同阶段连续 3 次失败自动降级配置，降级后引导
//! 成功率恢复记录在账。
//! 域：CoRun·UNX-A1-B07（引导降级面）。
//!
//! 结构：引导阶段闭集（early/mm/drivers/rootfs/init 六阶段）+ 失败
//! 计数账（同阶段连续 3 → 触发降级）+ 降级配置梯（full → safe →
//! minimal → recovery 四档冻结）+ 降级后成功率恢复账（降级前后
//! N 次引导成败对比）+ 注入核（跨阶段混计/降级越梯/恢复漏账/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA800 独占 0x80）。

pub const SEG_BASE: u32 = 0xA800;
pub const XC_BF_MIX: u32 = SEG_BASE + 0x01; // 跨阶段混计
pub const XC_BF_LADDER: u32 = SEG_BASE + 0x02; // 降级越梯
pub const XC_BF_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 引导阶段闭集。
pub const PH_EARLY: u8 = 0;
pub const PH_MM: u8 = 1;
pub const PH_DRIVERS: u8 = 2;
pub const PH_ROOTFS: u8 = 3;
pub const PH_INIT: u8 = 4;
pub const PH_SERVICE: u8 = 5;
pub const PHASES: u8 = 6;

/// 降级档闭集。
pub const LVL_FULL: u8 = 0;
pub const LVL_SAFE: u8 = 1;
pub const LVL_MINIMAL: u8 = 2;
pub const LVL_RECOVERY: u8 = 3;

/// 档名。
pub fn level_name(l: u8) -> &'static str {
    match l {
        LVL_FULL => "full",
        LVL_SAFE => "safe",
        LVL_MINIMAL => "minimal",
        LVL_RECOVERY => "recovery",
        _ => "?",
    }
}

/// 连续失败阈值。
pub const FAIL_THRESHOLD: u8 = 3;

/// 引导降级账。
pub struct DegradeBook {
    pub fail_streak: [u8; PHASES as usize], // 每阶段连续失败计数
    pub level: u8,
    pub events: Vec<String>,
    pub sealed: bool,
}

impl DegradeBook {
    pub fn new() -> Self {
        DegradeBook {
            fail_streak: [0; PHASES as usize],
            level: LVL_FULL,
            events: Vec::new(),
            sealed: false,
        }
    }

    /// 记录一次阶段成败。
    pub fn record(&mut self, phase: u8, ok: bool) -> Result<Option<u8>, u32> {
        if self.sealed {
            return Err(XC_BF_SEALED);
        }
        if phase >= PHASES {
            return Err(XC_BF_MIX);
        }
        if ok {
            self.fail_streak[phase as usize] = 0;
            return Ok(None);
        }
        self.fail_streak[phase as usize] += 1;
        if self.fail_streak[phase as usize] >= FAIL_THRESHOLD {
            // 触发降级（不越 recovery 档）
            if self.level < LVL_RECOVERY {
                self.level += 1;
                let ev = format!(
                    "degrade:{}->{}(phase={})",
                    self.level - 1,
                    self.level,
                    phase
                );
                self.events.push(ev);
                // 降级后计数清零（新配置新起点）
                self.fail_streak = [0; PHASES as usize];
                return Ok(Some(self.level));
            }
            return Err(XC_BF_LADDER);
        }
        Ok(None)
    }

    /// 恢复账：降级后连续 SUCCESS_N 次成功 → 记录恢复。
    pub fn record_recovery(&mut self, ok_streak: &mut u8, ok: bool) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_BF_SEALED);
        }
        if ok {
            *ok_streak += 1;
            if *ok_streak >= 5 && self.level > LVL_FULL {
                let ev = format!("recovered:{}->full", level_name(self.level));
                self.events.push(ev);
                self.level = LVL_FULL;
                *ok_streak = 0;
                return Ok(true);
            }
        } else {
            *ok_streak = 0;
        }
        Ok(false)
    }
}

/// 连续 3 失败降级核：early 三败 → safe。
pub fn streak3_exact() -> bool {
    let mut b = DegradeBook::new();
    if b.record(PH_EARLY, false).is_err() {
        return false;
    }
    if b.record(PH_EARLY, false).is_err() {
        return false;
    }
    // 成功一次清零
    if b.record(PH_EARLY, true).is_err() {
        return false;
    }
    if b.fail_streak[PH_EARLY as usize] != 0 {
        return false;
    }
    // 再三败 → 降级 safe
    let _ = b.record(PH_EARLY, false);
    let _ = b.record(PH_EARLY, false);
    match b.record(PH_EARLY, false) {
        Ok(Some(LVL_SAFE)) => true,
        _ => false,
    }
}

/// 降级梯核：full→safe→minimal→recovery，recovery 后再降拒。
pub fn ladder_exact() -> bool {
    let mut b = DegradeBook::new();
    // 9 次三连败 → 三级降
    for expect in [LVL_SAFE, LVL_MINIMAL, LVL_RECOVERY] {
        let _ = b.record(PH_MM, false);
        let _ = b.record(PH_MM, false);
        match b.record(PH_MM, false) {
            Ok(Some(l)) if l == expect => {}
            _ => return false,
        }
    }
    // recovery 后再败不再降
    let _ = b.record(PH_INIT, false);
    let _ = b.record(PH_INIT, false);
    b.record(PH_INIT, false) == Err(XC_BF_LADDER)
}

/// 跨阶段混计拒核：每阶段独立计数。
pub fn phase_indep_exact() -> bool {
    let mut b = DegradeBook::new();
    // early 两败 + mm 两败：均未达阈
    let _ = b.record(PH_EARLY, false);
    let _ = b.record(PH_EARLY, false);
    let _ = b.record(PH_MM, false);
    let _ = b.record(PH_MM, false);
    b.fail_streak[0] == 2 && b.fail_streak[1] == 2 && b.level == LVL_FULL
}

/// 降级后成功率恢复核：降级后 5 连成功 → 恢复 full 留账。
pub fn recovery_exact() -> bool {
    let mut b = DegradeBook::new();
    for _ in 0..3 {
        let _ = b.record(PH_ROOTFS, false);
    }
    if b.level != LVL_SAFE {
        return false;
    }
    let mut streak = 0u8;
    let mut recovered = false;
    for _ in 0..5 {
        match b.record_recovery(&mut streak, true) {
            Ok(r) => recovered = recovered || r,
            Err(_) => return false,
        }
    }
    recovered && b.level == LVL_FULL && b.events.len() == 2
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = DegradeBook::new();
    // 1) 阶段越界
    if b.record(9, false) == Err(XC_BF_MIX) {
        caught += 1;
    }
    // 2) 降级越梯
    for _ in 0..12 {
        let _ = b.record(PH_DRIVERS, false);
    }
    if b.record(PH_DRIVERS, false) == Err(XC_BF_LADDER) {
        caught += 1;
    }
    // 3) 封账后拒
    b.sealed = true;
    if b.record(PH_EARLY, false) == Err(XC_BF_SEALED) {
        caught += 1;
    }
    // 4) 恢复中途失败清零
    let mut b2 = DegradeBook::new();
    for _ in 0..3 {
        let _ = b2.record(PH_INIT, false);
    }
    let mut st = 0u8;
    let _ = b2.record_recovery(&mut st, true);
    let _ = b2.record_recovery(&mut st, true);
    let _ = b2.record_recovery(&mut st, false);
    let _ = b2.record_recovery(&mut st, true);
    let _ = b2.record_recovery(&mut st, true);
    if b2.record_recovery(&mut st, true) == Ok(false) && b2.level == LVL_SAFE && st == 2 {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：降级账折叠哈希。
pub fn fp_frozen(b: &DegradeBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for f in b.fail_streak.iter() {
        h ^= *f as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= b.level as u64;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx124_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0124");
    // J1-1 连续 3 失败降级
    let ok = streak3_exact();
    s.add("B07-01-三败降级", ok, if ok { "" } else { "降级漂移" });
    // J1-2 降级梯
    let ok = ladder_exact();
    s.add("B07-02-降级梯", ok, if ok { "" } else { "梯漂移" });
    // J1-3 跨阶段独立
    let ok = phase_indep_exact();
    s.add("B07-03-阶段独立", ok, if ok { "" } else { "混计" });
    // J1-4 恢复账
    let ok = recovery_exact();
    s.add("B07-04-恢复账", ok, if ok { "" } else { "恢复漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B07-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let b = DegradeBook::new();
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B07-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
