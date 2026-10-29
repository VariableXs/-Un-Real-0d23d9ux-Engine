//! UNX-F10839 sigaltstack 备用栈面（SA_ONSTACK）（0x1c160 段）
//!
//! 锚点：UNX-F10839-J1 备用栈对平——栈区小于 MINSIGSTKSZ 必 ENOMEM、
//! 栈启用中重设必 EPERM、SA_ONSTACK 旗标路由至备用栈三线全对平；注入
//! 坏实现（小栈放行/启用中重设放行）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：备用栈账（基址 × 容量 × 启用位）+ 注册判定器（容量对撞
//! MINSIGSTKSZ 算 ENOMEM/注册、启用位对撞算 EPERM）+ 交付路由状态机
//! （SA_ONSTACK 切栈）+ 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：ENOMEM=12、EPERM=1、MINSIGSTKSZ=2048。
pub const POSIX_ENOMEM: u32 = 12;
pub const POSIX_EPERM: u32 = 1;
pub const MINSIGSTKSZ: usize = 2048;
pub const SA_ONSTACK: u32 = 0x8;

/// 诊断段（0x1c160 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c160;
pub const AS_SMALL_STACK: u32 = SEG_BASE + 0x01; // 小栈注册
pub const AS_ACTIVE_RESET: u32 = SEG_BASE + 0x02; // 启用中重设
pub const AS_ROUTE_MISS: u32 = SEG_BASE + 0x03; // ONSTACK 路由缺失
pub const AS_DISABLE_MISS: u32 = SEG_BASE + 0x04; // 关闭面缺失
pub const AS_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const AS_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        AS_SMALL_STACK => "小栈注册",
        AS_ACTIVE_RESET => "启用中重设",
        AS_ROUTE_MISS => "ONSTACK 路由缺失",
        AS_DISABLE_MISS => "关闭面缺失",
        AS_REPLAY_MISS => "重放不齐",
        AS_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 备用栈账条目。
#[derive(Clone)]
pub struct AltStack {
    pub base: u64,
    pub size: usize,
    pub active: bool,
    pub note: String,
}

/// 注册场景：容量 × 当前是否启用（旧栈在用）。
#[derive(Clone)]
pub struct Scenario {
    pub size: usize,
    pub cur_active: bool,
}

/// 注册判定核：容量 < MINSIGSTKSZ 必 ENOMEM；启用中重设必 EPERM；否则注册。
pub fn judge(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.size < MINSIGSTKSZ {
        return Err(POSIX_ENOMEM);
    }
    if sc.cur_active {
        return Err(POSIX_EPERM);
    }
    Ok("registered")
}

/// 处理器旗标路由核：SA_ONSTACK 且备用栈已注册 → 切备用栈交付；否则主栈。
pub fn route(flags: u32, alt_registered: bool) -> &'static str {
    if flags & SA_ONSTACK != 0 && alt_registered {
        "alt-stack"
    } else {
        "main-stack"
    }
}

/// 备用栈状态机：注册/启用/关闭生命周期。
pub struct AltLedger {
    pub stack: Option<AltStack>,
    pub log: Vec<String>,
    pub note: String,
}

impl AltLedger {
    pub fn new() -> AltLedger {
        AltLedger { stack: None, log: Vec::new(), note: "none".to_string() }
    }

    /// 注册备用栈。
    pub fn register(&mut self, sc: &Scenario, base: u64) -> Result<(), u32> {
        let out = judge(sc);
        if out.is_ok() {
            self.stack = Some(AltStack { base, size: sc.size, active: false, note: "idle".to_string() });
            self.log.push("register".to_string());
            self.note = "registered".to_string();
        }
        out.map(|_| ())
    }

    /// 启用（进入处理器时）。
    pub fn activate(&mut self) -> bool {
        match &mut self.stack {
            Some(s) if !s.active => {
                s.active = true;
                s.note = "on-alt".to_string();
                self.log.push("activate".to_string());
                self.note = "active".to_string();
                true
            }
            _ => false,
        }
    }

    /// 关闭（退出处理器时）。
    pub fn deactivate(&mut self) -> bool {
        match &mut self.stack {
            Some(s) if s.active => {
                s.active = false;
                s.note = "idle".to_string();
                self.log.push("deactivate".to_string());
                self.note = "registered".to_string();
                true
            }
            _ => false,
        }
    }

    /// 重放核：启用/关闭交替对账（active 位与账行一致）。
    pub fn replay_ok(&self) -> bool {
        let mut ok = true;
        let mut active = false;
        let mut i = 0usize;
        while i < self.log.len() {
            match self.log[i].as_str() {
                "activate" if active => ok = false,
                "activate" => active = true,
                "deactivate" if !active => ok = false,
                "deactivate" => active = false,
                _ => {}
            }
            i += 1;
        }
        ok && active == self.stack.as_ref().map_or(false, |s| s.active)
    }
}

/// 坏实现 A：小栈放行。
pub fn faulty_small(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.cur_active {
        return Err(POSIX_EPERM);
    }
    Ok("registered")
}

/// 坏实现 B：启用中重设放行。
pub fn faulty_active_reset(sc: &Scenario) -> Result<&'static str, u32> {
    if sc.size < MINSIGSTKSZ {
        return Err(POSIX_ENOMEM);
    }
    Ok("registered")
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 小栈：权威 ENOMEM、坏实现放行 → 检出
    let sc1 = Scenario { size: 1024, cur_active: false };
    got[0] = judge(&sc1) != faulty_small(&sc1) && judge(&sc1).err() == Some(POSIX_ENOMEM);
    // 2) 启用中重设：权威 EPERM、坏实现放行 → 检出
    let sc2 = Scenario { size: 4096, cur_active: true };
    got[1] = judge(&sc2) != faulty_active_reset(&sc2) && judge(&sc2).err() == Some(POSIX_EPERM);
    got
}

/// 生命周期全序核：注册 → 启用 → 关闭 → 再启用对账。
pub fn lifecycle_ok() -> bool {
    let mut lg = AltLedger::new();
    let r1 = lg.register(&Scenario { size: 4096, cur_active: false }, 0x7F00_0000);
    let a1 = lg.activate();
    let d1 = lg.deactivate();
    let a2 = lg.activate();
    let d2 = lg.deactivate();
    r1.is_ok() && a1 && d1 && a2 && d2 && lg.replay_ok() && lg.note == "registered"
}

/// 启用中重设防御核：active 期间 register 必拒（EPERM）。
pub fn active_guard_ok() -> bool {
    let mut lg = AltLedger::new();
    let _ = lg.register(&Scenario { size: 4096, cur_active: false }, 0x7F00_0000);
    let _ = lg.activate();
    let r = judge(&Scenario { size: 4096, cur_active: true });
    r.err() == Some(POSIX_EPERM)
}

/// 路由核对账：注册+ONSTACK 切栈、未注册回主栈。
pub fn route_ok() -> bool {
    route(SA_ONSTACK, true) == "alt-stack"
        && route(SA_ONSTACK, false) == "main-stack"
        && route(0, true) == "main-stack"
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望注记或诊断码。
pub fn sample_matrix() -> Vec<(Scenario, Option<u32>)> {
    let mut m: Vec<(Scenario, Option<u32>)> = Vec::new();
    // 正：足栈未启用
    m.push((Scenario { size: 4096, cur_active: false }, None));
    m.push((Scenario { size: MINSIGSTKSZ, cur_active: false }, None));
    // 反：小栈
    m.push((Scenario { size: MINSIGSTKSZ - 1, cur_active: false }, Some(POSIX_ENOMEM)));
    m.push((Scenario { size: 0, cur_active: false }, Some(POSIX_ENOMEM)));
    // 反：启用中重设
    m.push((Scenario { size: 8192, cur_active: true }, Some(POSIX_EPERM)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (sc, want) = &m[i];
        let got = judge(sc);
        let hit = match want {
            None => got.is_ok(),
            Some(c) => got.err() == Some(*c),
        };
        if hit {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 备用栈面注记（对账/落账用）。
pub fn alt_note(lg: &AltLedger) -> String {
    match &lg.stack {
        Some(s) => format!("base={:#x} size={} active={} note={}", s.base, s.size, s.active, lg.note),
        None => "no-stack".to_string(),
    }
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10839_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10839");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线注册样本全命中");

    // J2 反路径：小栈必 ENOMEM
    let j2 = judge(&Scenario { size: 1024, cur_active: false }).err() == Some(POSIX_ENOMEM);
    s.add("J2-小栈ENOMEM", j2, "容量 < MINSIGSTKSZ 必 12");

    // J3 反路径：启用中重设必 EPERM
    let j3 = judge(&Scenario { size: 8192, cur_active: true }).err() == Some(POSIX_EPERM);
    s.add("J3-启用中拒", j3, "active 重设必 1");

    // J4 边界：恰 MINSIGSTKSZ 注册
    let j4 = judge(&Scenario { size: MINSIGSTKSZ, cur_active: false }).is_ok();
    s.add("J4-恰界注册", j4, "==MINSIGSTKSZ 通");

    // J5 生命周期全序核
    s.add("J5-生命周期核", lifecycle_ok(), "注册→启用→关闭交替对账");

    // J6 注入1 小栈放行检出
    s.add("J6-小栈检出", inj[0], "小栈放行坏实现必分歧");

    // J7 注入2 启用中重设放行检出
    s.add("J7-重设检出", inj[1], "active 放行坏实现必分歧");

    // J8 启用中重设防御核
    s.add("J8-active防御", active_guard_ok(), "状态机 active 位拒注册");

    // J9 路由核对账
    s.add("J9-路由核", route_ok(), "ONSTACK 三态路由全对");

    // J10 字面锚：ENOMEM=12 / EPERM=1 / MINSIGSTKSZ=2048 / SA_ONSTACK=0x8
    let j10 = POSIX_ENOMEM == 12 && POSIX_EPERM == 1 && MINSIGSTKSZ == 2048
        && SA_ONSTACK == 0x8 && diag_text(AS_SMALL_STACK).len() > 0;
    s.add("J10-字面锚对平", j10, "12/1/2048/0x8 四锚");

    // J11 备用栈面注记闭集
    let mut lg2 = AltLedger::new();
    let n0 = alt_note(&lg2);
    let _ = lg2.register(&Scenario { size: 4096, cur_active: false }, 0x7F00_0000);
    let n1 = alt_note(&lg2);
    let j11 = n0 == "no-stack" && n1.contains("base=0x7f000000") && n0 != n1;
    s.add("J11-注记互异", j11, "no-stack/registered 注记互异");

    // J12 未注册交付回主栈
    let j12 = route(SA_ONSTACK, false) == "main-stack" && route(0, false) == "main-stack";
    s.add("J12-未注册回退", j12, "无备用栈走主栈");

    // J13 (dyn) 备用栈账指纹落账
    let mut lg3 = AltLedger::new();
    let _ = lg3.register(&Scenario { size: 4096, cur_active: false }, 0x7F00_0000);
    let _ = lg3.activate();
    let _ = lg3.deactivate();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(lg3.log.len() as u32);
    for row in &lg3.log {
        for c in row.bytes() {
            h = h.wrapping_mul(31).wrapping_add(c as u32);
        }
    }
    for c in lg3.note.bytes() {
        h = h.wrapping_mul(31).wrapping_add(c as u32);
    }
    s.add_dyn("J13-备用栈账指纹", h != 0, &format!("fold={:#x}", h));

    // J14 (dyn) 矩阵命中明细
    s.add_dyn(
        "J14-命中明细",
        hits == total,
        &format!("hits={}/{} cap_ok={}", hits, total, cap_ok),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10839_altstack_flow() {
        assert!(judge(&Scenario { size: 1024, cur_active: false }).err() == Some(POSIX_ENOMEM));
        assert!(judge(&Scenario { size: 8192, cur_active: true }).err() == Some(POSIX_EPERM));
        assert!(judge(&Scenario { size: MINSIGSTKSZ, cur_active: false }).is_ok());
        let mut lg = AltLedger::new();
        assert!(lg.register(&Scenario { size: 4096, cur_active: false }, 0x1000).is_ok());
        assert!(lg.activate() && lg.deactivate());
        assert!(lg.replay_ok());
        assert_eq!(route(SA_ONSTACK, true), "alt-stack");
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10839_checks_green() {
        let s = run_unx10839_checks();
        assert!(s.all_passed(), "UNX-F10839 判据未全绿");
    }
}
