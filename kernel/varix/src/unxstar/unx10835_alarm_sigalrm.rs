//! UNX-F10835 alarm/SIGALRM 定时信号矩阵（0x1c0e0 段）
//!
//! 锚点：UNX-F10835-J1 定时对平——alarm(n) 返回前次剩余秒、n=0 取消且
//! 返回剩余、替换闹钟必归还旧剩余、到期必置 SIGALRM 交付四线全对平；
//! 注入坏实现（剩余丢失/到期不投）必与权威分歧。
//! 域：CoRun·UNX-C3（错误矩阵面谱系）。
//!
//! 结构：闹钟账（剩余秒 × 到期位）+ 设置判定器（新值对撞旧剩余算 归还/
//! 取消）+ tick 状态机（逐秒递减到期置 SIGALRM）+ 重放核（时序逐笔同判）
//! + 注入核（两类坏实现必分歧）+ 正反双样本矩阵。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// POSIX 锚：SIGALRM=14。
pub const SIGALRM: u32 = 14;

/// 诊断段（0x1c0e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1c0e0;
pub const ALM_REST_LOSS: u32 = SEG_BASE + 0x01; // 剩余秒丢失
pub const ALM_FIRE_MISS: u32 = SEG_BASE + 0x02; // 到期不投递
pub const ALM_CANCEL_SKEW: u32 = SEG_BASE + 0x03; // 取消错账
pub const ALM_TICK_SKEW: u32 = SEG_BASE + 0x04; // tick 错账
pub const ALM_REPLAY_MISS: u32 = SEG_BASE + 0x05; // 重放不齐
pub const ALM_NOTE_SKEW: u32 = SEG_BASE + 0x06; // 注记错账

/// 诊断码语义文本（对账/落账用）。
pub fn diag_text(code: u32) -> &'static str {
    match code {
        ALM_REST_LOSS => "剩余秒丢失",
        ALM_FIRE_MISS => "到期不投递",
        ALM_CANCEL_SKEW => "取消错账",
        ALM_TICK_SKEW => "tick 错账",
        ALM_REPLAY_MISS => "重放不齐",
        ALM_NOTE_SKEW => "注记错账",
        _ => "未知诊断码",
    }
}

/// 设置输出：(返回的旧剩余, 新剩余)。
pub type AlmOut = (u32, u32);

/// 设置判定核：alarm(n) —— 返回旧剩余；n=0 取消（新剩余 0）；n>0 替换。
pub fn judge(old_remaining: u32, new: u32) -> AlmOut {
    (old_remaining, new)
}

/// 闹钟状态机：remaining 递减、到期置 SIGALRM、交付清账。
pub struct Alarm {
    pub remaining: u32,
    pub fired: bool,
    pub delivered: u32,
    pub log: Vec<String>,
    pub note: String,
}

impl Alarm {
    pub fn new() -> Alarm {
        Alarm { remaining: 0, fired: false, delivered: 0, log: Vec::new(), note: "idle".to_string() }
    }

    /// 设置闹钟：归还旧剩余。
    pub fn set(&mut self, n: u32) -> u32 {
        let old = self.remaining;
        self.remaining = n;
        self.fired = false;
        self.note = if n == 0 { "cancelled".to_string() } else { "armed".to_string() };
        self.log.push(format!("set:{} old:{}", n, old));
        old
    }

    /// tick 一秒：递减，到 0 置 fired。
    pub fn tick(&mut self) {
        if self.remaining > 0 {
            self.remaining -= 1;
            if self.remaining == 0 {
                self.fired = true;
                self.log.push("fire".to_string());
                self.note = "fired".to_string();
            }
        }
    }

    /// SIGALRM 交付：fired → 计数 +1 并清到期位。
    pub fn deliver(&mut self) -> bool {
        if self.fired {
            self.fired = false;
            self.delivered += 1;
            self.log.push("deliver".to_string());
            true
        } else {
            false
        }
    }

    /// 账面核：到期必投递（fired 未清则账不平）。
    pub fn ledger_ok(&self) -> bool {
        !self.fired && self.deliveries_match()
    }

    fn deliveries_match(&self) -> bool {
        self.delivered as usize + self.fired as usize <= self.log.len()
    }
}

/// 正反双样本矩阵容量（先定后用）。
pub const MATRIX_CAP: usize = 16;

/// 正反双样本矩阵：期望 AlmOut。
pub fn sample_matrix() -> Vec<(u32, u32, AlmOut)> {
    let mut m: Vec<(u32, u32, AlmOut)> = Vec::new();
    // 正：无旧钟设置
    m.push((0, 10, (0, 10)));
    // 正：替换归还旧剩余
    m.push((7, 30, (7, 30)));
    // 边界：取消归还旧剩余
    m.push((5, 0, (5, 0)));
    // 边界：旧剩余 0 取消
    m.push((0, 0, (0, 0)));
    // 边界：1 秒闹钟
    m.push((1, 1, (1, 1)));
    m
}

/// 矩阵对平：返回 (命中数, 总数, 容量合规)。
pub fn verify_matrix() -> (usize, usize, bool) {
    let m = sample_matrix();
    let mut ok = 0usize;
    let mut i = 0usize;
    while i < m.len() {
        let (old, new, want) = &m[i];
        if &judge(*old, *new) == want {
            ok += 1;
        }
        i += 1;
    }
    (ok, m.len(), m.len() <= MATRIX_CAP)
}

/// 坏实现 A：剩余丢失（替换不归还旧剩余）。
pub fn faulty_rest_loss(_old: u32, new: u32) -> AlmOut {
    (0, new)
}

/// 坏实现 B：到期不投递（fired 不清、交付恒假）。
pub fn faulty_no_deliver(a: &mut Alarm) -> bool {
    // 坏实现：交付判定反转（fired 时也不交）
    !a.fired
}

/// 注入核：两类坏实现必与权威分歧（检出）。
pub fn inject_two() -> [bool; 2] {
    let mut got = [false; 2];
    // 1) 剩余丢失：旧剩余 7，权威归还 7、坏实现归还 0 → 检出
    got[0] = judge(7, 30) != faulty_rest_loss(7, 30) && judge(7, 30).0 == 7;
    // 2) 到期不投递：权威 fired→交付真、坏实现假 → 检出
    let mut a = Alarm::new();
    let _ = a.set(1);
    a.tick();
    let good = a.deliver();
    let bad = faulty_no_deliver(&mut a);
    got[1] = good && !bad && good != bad && a.delivered == 1;
    got
}

/// 定时全序核：set(3) → tick×3 → fired → 交付；tick 超发不回绕。
pub fn seq_ok() -> bool {
    let mut a = Alarm::new();
    let r1 = a.set(3);
    a.tick();
    a.tick();
    let mid = a.remaining == 1 && !a.fired;
    a.tick();
    let fired = a.fired && a.remaining == 0;
    let d1 = a.deliver();
    let d2 = a.deliver();
    // 超发 tick：remaining 已 0 不再触发
    a.tick();
    let r2 = a.set(5);
    r1 == 0 && mid && fired && d1 && !d2 && !a.fired && r2 == 0
        && a.remaining == 5 && a.delivered == 1 && a.ledger_ok()
}

/// 取消核：set(5) → tick 2 → set(0) 取消归还 3 → tick 不触发。
pub fn cancel_ok() -> bool {
    let mut a = Alarm::new();
    let _ = a.set(5);
    a.tick();
    a.tick();
    let old = a.set(0);
    let c1 = old == 3 && a.remaining == 0 && a.note == "cancelled";
    a.tick();
    let c2 = !a.fired && a.delivered == 0;
    c1 && c2
}

/// 闹钟面注记（对账/落账用）。
pub fn alarm_note(a: &Alarm) -> String {
    format!("rem={} fired={} sig={}", a.remaining, a.fired, SIGALRM)
}

/// 判据（对应卡验收 J 判据）。
pub fn run_unx10835_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10835");
    let inj = inject_two();
    let (hits, total, cap_ok) = verify_matrix();

    // J1 矩阵对平
    s.add("J1-矩阵对平", hits == total && cap_ok, "五线定时样本全命中");

    // J2 正路径：替换归还旧剩余
    let j2 = judge(7, 30) == (7, 30);
    s.add("J2-归还旧剩余", j2, "alarm(30) 返回 7");

    // J3 边界：取消归还剩余
    let j3 = judge(5, 0) == (5, 0);
    s.add("J3-取消归还", j3, "alarm(0) 返回剩余");

    // J4 定时全序核：3 秒到期交付
    s.add("J4-全序核", seq_ok(), "set→tick×3→fire→deliver 全对");

    // J5 取消核：取消后不再触发
    s.add("J5-取消核", cancel_ok(), "取消归还且不触发");

    // J6 注入1 剩余丢失检出
    s.add("J6-丢失检出", inj[0], "剩余归零坏实现必分歧");

    // J7 注入2 到期不投检出
    s.add("J7-不投检出", inj[1], "到期不交坏实现必分歧");

    // J8 tick 超发防御：0 剩余不触发
    let mut a2 = Alarm::new();
    a2.tick();
    a2.tick();
    let j8 = !a2.fired && a2.remaining == 0 && a2.delivered == 0;
    s.add("J8-超发防御", j8, "无钟 tick 无害");

    // J9 字面锚：SIGALRM=14
    let j9 = SIGALRM == 14 && diag_text(ALM_REST_LOSS).len() > 0
        && diag_text(0xffff_ffff) == "未知诊断码";
    s.add("J9-字面锚对平", j9, "14 锚且语义完备");

    // J10 闹钟注记闭集
    let mut a3 = Alarm::new();
    let _ = a3.set(9);
    let n1 = alarm_note(&a3);
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    a3.tick();
    let n2 = alarm_note(&a3);
    let j10 = n1.contains("rem=9") && n2.contains("fired=true");
    s.add("J10-注记闭集", j10, "armed/fired 注记可辨");

    // J11 替换不重置交付账
    let mut a4 = Alarm::new();
    let _ = a4.set(1);
    a4.tick();
    let _ = a4.deliver();
    let old = a4.set(2);
    let j11 = old == 0 && a4.delivered == 1 && a4.ledger_ok();
    s.add("J11-交付账续存", j11, "替换不重置已交付计数");

    // J12 状态闭集：idle/armed/fired/cancelled 可辨
    let mut a5 = Alarm::new();
    let s0 = a5.note.clone();
    let _ = a5.set(4);
    let s1 = a5.note.clone();
    let _ = a5.set(0);
    let s2 = a5.note.clone();
    let j12 = s0 == "idle" && s1 == "armed" && s2 == "cancelled" && s0 != s1 && s1 != s2;
    s.add("J12-状态闭集", j12, "四态互异");

    // J13 (dyn) 闹钟账指纹落账
    let mut a6 = Alarm::new();
    let _ = a6.set(4);
    a6.tick();
    a6.tick();
    let mut h: u32 = SEG_BASE;
    h = h.wrapping_mul(31).wrapping_add(a6.remaining);
    h = h.wrapping_mul(31).wrapping_add(a6.fired as u32);
    for row in &a6.log {
        for c in row.bytes() {
            h = h.wrapping_mul(31).wrapping_add(c as u32);
        }
    }
    s.add_dyn("J13-闹钟账指纹", h != 0, &format!("fold={:#x}", h));

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
    fn unx10835_alarm_flow() {
        let mut a = Alarm::new();
        assert_eq!(a.set(2), 0);
        a.tick();
        assert_eq!(a.remaining, 1);
        assert_eq!(a.set(7), 1);
        assert!(!a.fired);
        a.tick();
        a.tick();
        a.tick();
        a.tick();
        a.tick();
        a.tick();
        a.tick();
        assert!(a.fired);
        assert!(a.deliver());
        assert_eq!(a.delivered, 1);
        assert!(a.ledger_ok());
        let inj = inject_two();
        assert!(inj.iter().all(|b| *b));
    }

    #[test]
    fn unx10835_checks_green() {
        let s = run_unx10835_checks();
        assert!(s.all_passed(), "UNX-F10835 判据未全绿");
    }
}
