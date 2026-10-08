//! F180 断电演练 · 批次八（v8）——演练资格门、并发演练互斥、
//! 恢复时间序列账、演练季度汇总帧。零堆、no_std。

use crate::checks::CheckSet;

/// 资格检查项数。
pub const ELIGIBILITY_CHECKS: usize = 5;
/// 季度汇总夜数上限。
pub const QUARTER_NIGHTS: usize = 24;
/// 季度帧长（14B）。
pub const QUARTER_FRAME_LEN: usize = 14;

/// 演练资格门：五查全过才可演练（盘健康/电池/温度/无并发任务/非静默期）。
#[derive(Clone, Copy)]
pub struct EligibilityGate {
    passed: [bool; ELIGIBILITY_CHECKS],
    names_set: [bool; ELIGIBILITY_CHECKS],
}

impl EligibilityGate {
    pub const fn new() -> EligibilityGate {
        EligibilityGate { passed: [false; ELIGIBILITY_CHECKS], names_set: [false; ELIGIBILITY_CHECKS] }
    }

    /// 设一查结果（index 0..5：盘/电池/温度/并发/静默期）。
    pub fn set(&mut self, idx: usize, passed: bool) -> bool {
        if idx >= ELIGIBILITY_CHECKS {
            return false;
        }
        self.passed[idx] = passed;
        self.names_set[idx] = true;
        true
    }

    /// 资格：五查全设且全过（缺查 = 未验 = 不过——fail-closed）。
    pub fn eligible(&self) -> bool {
        self.names_set.iter().all(|s| *s) && self.passed.iter().all(|p| *p)
    }

    /// 首个未过项（人话建议的定位面）。
    pub fn first_fail(&self) -> Option<usize> {
        if !self.names_set.iter().all(|s| *s) {
            return self.names_set.iter().position(|s| !s);
        }
        self.passed.iter().position(|p| !p)
    }

    /// 未设查数（资格判定的完备性对账）。
    pub fn unset_count(&self) -> u32 {
        self.names_set.iter().filter(|s| !**s).count() as u32
    }
}

/// 并发演练互斥：同一时刻只允许一个演练会话（两个演练同时跑 = 既耗电又失真）。
#[derive(Clone, Copy)]
pub struct DrillMutex {
    holder: Option<u32>,
    pub rejected: u32,
}

impl DrillMutex {
    pub const fn new() -> DrillMutex {
        DrillMutex { holder: None, rejected: 0 }
    }

    pub fn acquire(&mut self, session_id: u32) -> bool {
        if self.holder.is_some() {
            self.rejected += 1;
            return false;
        }
        self.holder = Some(session_id);
        true
    }

    /// 释放：仅持有者可放（他人释放 = 越权——拒绝）。
    pub fn release(&mut self, session_id: u32) -> bool {
        if self.holder == Some(session_id) {
            self.holder = None;
            true
        } else {
            false
        }
    }

    pub fn is_locked(&self) -> bool {
        self.holder.is_some()
    }
}

/// 恢复时间序列：环记夜号 → 恢复 ms；趋势 = 首末两点斜率（与 b7 同族语义）。
#[derive(Clone, Copy)]
pub struct RecoverySeries {
    nights: [u32; 16],
    ms: [u32; 16],
    pub n: usize,
}

impl RecoverySeries {
    pub const fn new() -> RecoverySeries {
        RecoverySeries { nights: [0; 16], ms: [0; 16], n: 0 }
    }

    pub fn record(&mut self, night: u32, recover_ms: u32) {
        if self.n < 16 {
            self.nights[self.n] = night;
            self.ms[self.n] = recover_ms;
            self.n += 1;
        }
    }

    /// 恶化检出：末值比首值高 50% 以上 → true（恢复在变慢——早警）。
    /// 样本 <3 → None 不猜。
    pub fn worsening(&self) -> Option<bool> {
        if self.n < 3 {
            return None;
        }
        let first = self.ms[0] as u64;
        let last = self.ms[self.n - 1] as u64;
        Some(last * 2 > first * 3)
    }

    /// 均值。
    pub fn mean_ms(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let sum: u64 = self.ms.iter().take(self.n).map(|&m| m as u64).sum();
        Some((sum / self.n as u64) as u32)
    }

    /// 最差夜定位。
    pub fn worst_night(&self) -> Option<(u32, u32)> {
        if self.n == 0 {
            return None;
        }
        let mut bi = 0;
        for i in 1..self.n {
            if self.ms[i] > self.ms[bi] {
                bi = i;
            }
        }
        Some((self.nights[bi], self.ms[bi]))
    }
}

/// 季度汇总帧（14B）：
/// [0..2) "DQ" · [2..4) 季度号 LE · [4..6) 应跑夜数 LE · [6..8) 实跑 LE ·
/// [8..10) 全过夜 LE · [10..12) 均恢复 ms/10 LE · [12..14) 校验和（前 12B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_quarter(quarter: u16, planned: u16, ran: u16, all_green: u16, mean_ms: u32, out: &mut [u8; QUARTER_FRAME_LEN]) -> bool {
    if ran > planned || all_green > ran {
        return false; // 实跑 > 应跑 / 全过 > 实跑 = 账不平
    }
    out[0] = b'D';
    out[1] = b'Q';
    out[2..4].copy_from_slice(&quarter.to_le_bytes());
    out[4..6].copy_from_slice(&planned.to_le_bytes());
    out[6..8].copy_from_slice(&ran.to_le_bytes());
    out[8..10].copy_from_slice(&all_green.to_le_bytes());
    out[10..12].copy_from_slice(&((mean_ms / 10) as u16).to_le_bytes());
    let c = fnv16(&out[..12]);
    out[12] = (c & 0xFF) as u8;
    out[13] = (c >> 8) as u8;
    true
}

pub fn decode_quarter(frame: &[u8; QUARTER_FRAME_LEN]) -> Option<(u16, u16, u16, u16, u32)> {
    if frame[0] != b'D' || frame[1] != b'Q' {
        return None;
    }
    let want = (frame[13] as u16) << 8 | frame[12] as u16;
    if fnv16(&frame[..12]) != want {
        return None;
    }
    Some((
        u16::from_le_bytes(frame[2..4].try_into().ok()?),
        u16::from_le_bytes(frame[4..6].try_into().ok()?),
        u16::from_le_bytes(frame[6..8].try_into().ok()?),
        u16::from_le_bytes(frame[8..10].try_into().ok()?),
        u16::from_le_bytes(frame[10..12].try_into().ok()?) as u32 * 10,
    ))
}

#[inline(never)]
pub fn run_pwrdrill_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b8");

    // 1) 资格门正线：五查全过 → 有资格。
    let mut g = EligibilityGate::new();
    for i in 0..ELIGIBILITY_CHECKS {
        g.set(i, true);
    }
    cs.add("eligibility_all_pass", g.eligible() && g.first_fail().is_none(), "");

    // 2) 缺查即拒：四查过但一查未设 → 无资格（fail-closed——未验不算过）。
    let mut g2 = EligibilityGate::new();
    for i in 0..ELIGIBILITY_CHECKS - 1 {
        g2.set(i, true);
    }
    cs.add(
        "eligibility_unset_fails_closed",
        !g2.eligible() && g2.unset_count() == 1 && g2.first_fail() == Some(4),
        "",
    );

    // 3) 首败定位：全设但温度查败 → first_fail = 2（人话建议的锚点）。
    let mut g3 = EligibilityGate::new();
    g3.set(0, true);
    g3.set(1, true);
    g3.set(2, false);
    g3.set(3, true);
    g3.set(4, true);
    cs.add("eligibility_first_fail_located", !g3.eligible() && g3.first_fail() == Some(2), "");

    // 4) 资格越界设查拒：idx 5 → false（查表面守门）。
    cs.add("eligibility_oob_rejected", !g3.set(5, true), "");

    // 5) 互斥：A 持锁、B 被拒计数；A 放行后 B 可得。
    let mut m = DrillMutex::new();
    let a = m.acquire(1);
    let b = m.acquire(2);
    let bad_release = !m.release(2);
    m.release(1);
    let c = m.acquire(2);
    cs.add(
        "drill_mutex",
        a && !b && m.rejected == 1 && bad_release && c && !m.is_locked() == false,
        "",
    );

    // 6) 恢复序列：3 样本 5000→5200→5300 → 未恶化（5300*2=10600 ≤ 15000）。
    let mut r = RecoverySeries::new();
    r.record(1, 5_000);
    r.record(2, 5_200);
    r.record(3, 5_300);
    cs.add(
        "recovery_stable",
        r.worsening() == Some(false) && r.mean_ms() == Some(5_166),
        "",
    );

    // 7) 恶化检出：5000 → 9000（9000*2 > 5000*3）→ 早警（恢复在变慢）。
    let mut r2 = RecoverySeries::new();
    r2.record(1, 5_000);
    r2.record(2, 6_000);
    r2.record(3, 9_000);
    cs.add("recovery_worsening_detected", r2.worsening() == Some(true), "");

    // 8) 样本不足不猜：1 样本 → None；空账全 None。
    let mut r3 = RecoverySeries::new();
    r3.record(1, 4_000);
    let r4 = RecoverySeries::new();
    cs.add(
        "recovery_insufficient_honest",
        r3.worsening().is_none() && r4.worsening().is_none() && r4.mean_ms().is_none() && r4.worst_night().is_none(),
        "",
    );

    // 9) 最差夜定位：3 夜中第 2 夜 8s 最差 → (2, 8000)。
    cs.add("recovery_worst_located", r2.worst_night() == Some((3, 9_000)), "");

    // 10) 季度帧 round-trip：合法账平往返一致。
    let mut f = [0u8; QUARTER_FRAME_LEN];
    let ok = encode_quarter(3, 24, 22, 21, 5_180, &mut f);
    cs.add(
        "quarter_frame_roundtrip",
        ok && decode_quarter(&f) == Some((3, 24, 22, 21, 5_180)) && f[0] == b'D',
        "",
    );

    // 11) 季度帧账平守门：实跑 > 应跑 拒、全过 > 实跑 拒。
    let mut f2 = [0u8; QUARTER_FRAME_LEN];
    let bad1 = !encode_quarter(3, 10, 11, 5, 1_000, &mut f2);
    let bad2 = !encode_quarter(3, 10, 8, 9, 1_000, &mut f2);
    cs.add("quarter_frame_accounted", bad1 && bad2, "");

    // 12) 季度帧撕裂拒：14 字节逐一翻转全拦。
    let mut tear_ok = true;
    for i in 0..QUARTER_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x6D;
        if decode_quarter(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add("quarter_frame_tear_proof", tear_ok, "");

    // 13) 常量自洽：查 5、夜 24、帧 14。
    cs.add("b8_constants", ELIGIBILITY_CHECKS == 5 && QUARTER_NIGHTS == 24 && QUARTER_FRAME_LEN == 14, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutex_reacquire_after_release() {
        // 释放后可重获：锁是会话的，不是永久的。
        let mut m = DrillMutex::new();
        m.acquire(7);
        assert!(m.release(7));
        assert!(!m.is_locked());
        assert!(m.acquire(8));
    }

    #[test]
    fn recovery_series_cap_at_16() {
        // 16 上限守门：第 17 条拒收（环满语义——模型层诚实）。
        let mut r = RecoverySeries::new();
        for i in 0..20u32 {
            r.record(i, 1_000 + i);
        }
        assert_eq!(r.n, 16);
        assert_eq!(r.worst_night(), Some((15, 1_015)));
    }

    #[test]
    fn eligibility_partial_then_complete() {
        // 补齐后资格翻转：先缺查拒、补上后过（状态可演进）。
        let mut g = EligibilityGate::new();
        for i in 0..ELIGIBILITY_CHECKS - 1 {
            g.set(i, true);
        }
        assert!(!g.eligible());
        g.set(ELIGIBILITY_CHECKS - 1, true);
        assert!(g.eligible());
    }
}
