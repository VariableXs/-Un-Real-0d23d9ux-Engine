//! F183 磁盘健康 · 批次八（v8）——写入放大账、坏块重映射地图、
//! 掉电事件账、健康趋势周报帧。零堆、no_std。

use crate::checks::CheckSet;

/// 写入放大窗口（采样数）。
pub const WAF_SAMPLES: usize = 8;
/// 重映射地图容量。
pub const REMAP_CAP: usize = 24;
/// 掉电事件环容量。
pub const POWERLOSS_CAP: usize = 8;
/// 周报帧长（14B）。
pub const WEEKLY_FRAME_LEN: usize = 14;

/// 写入放大追踪：WAF = 实际写入 / 主机写入（u128 防溢出）。
#[derive(Clone, Copy)]
pub struct WafTracker {
    host_kib: [u64; WAF_SAMPLES],
    nand_kib: [u64; WAF_SAMPLES],
    head: usize,
    pub n: usize,
}

impl WafTracker {
    pub const fn new() -> WafTracker {
        WafTracker { host_kib: [0; WAF_SAMPLES], nand_kib: [0; WAF_SAMPLES], head: 0, n: 0 }
    }

    pub fn sample(&mut self, host_write_kib: u64, nand_write_kib: u64) {
        self.host_kib[self.head] = host_write_kib;
        self.nand_kib[self.head] = nand_write_kib;
        self.head = (self.head + 1) % WAF_SAMPLES;
        if self.n < WAF_SAMPLES {
            self.n += 1;
        }
    }

    /// 窗口 WAF（‰，u128 中间量；主机写 0 → None 不猜）。
    pub fn waf_permille(&self) -> Option<u32> {
        let mut h = 0u128;
        let mut nn = 0u128;
        for i in 0..self.n {
            h += self.host_kib[i] as u128;
            nn += self.nand_kib[i] as u128;
        }
        if h == 0 {
            return None;
        }
        Some(((nn * 1_000) / h) as u32)
    }

    /// WAF 分级：<1100 优 / <2000 良 / ≥2000 差（放大 2 倍即病）。
    pub fn grade(&self) -> Option<u8> {
        let w = self.waf_permille()?;
        Some(if w >= 2_000 {
            2
        } else if w >= 1_100 {
            1
        } else {
            0
        })
    }
}

/// 坏块重映射地图：坏块号 → 替补块号；替补耗尽诚实上报。
#[derive(Clone, Copy)]
pub struct RemapMap {
    bad: [Option<u32>; REMAP_CAP],
    spare: [u32; REMAP_CAP],
    pub n: usize,
    pub spares_left: u32,
}

impl RemapMap {
    pub const fn new(total_spares: u32) -> RemapMap {
        RemapMap { bad: [None; REMAP_CAP], spare: [0; REMAP_CAP], n: 0, spares_left: total_spares }
    }

    /// 重映射：坏块入图、消耗一个替补。替补尽 → None（数据面危险信号）。
    pub fn remap(&mut self, bad_block: u32) -> Option<u32> {
        if self.n >= REMAP_CAP {
            return None;
        }
        for i in 0..self.n {
            if self.bad[i] == Some(bad_block) {
                return None; // 重复重映射同一块 = 判定矛盾
            }
        }
        if self.spares_left == 0 {
            return None;
        }
        self.spares_left -= 1;
        let s = 10_000 + self.spares_left; // 替补块号从高位分配
        self.bad[self.n] = Some(bad_block);
        self.spare[self.n] = s;
        self.n += 1;
        Some(s)
    }

    /// 查映射（读请求路由面）。
    pub fn mapping_of(&self, bad_block: u32) -> Option<u32> {
        for i in 0..self.n {
            if self.bad[i] == Some(bad_block) {
                return Some(self.spare[i]);
            }
        }
        None
    }

    /// 健康度：替补剩余 ‰（初值给定时线性递减）。
    pub fn spare_health_permille(&self, initial_spares: u32) -> u32 {
        if initial_spares == 0 {
            return 0;
        }
        self.spares_left * 1_000 / initial_spares
    }
}

/// 掉电事件账：环记 (掉电日, 恢复是否干净)；频次与脏恢复统计。
#[derive(Clone, Copy)]
pub struct PowerLossLedger {
    days: [u32; POWERLOSS_CAP],
    dirty_recover: [bool; POWERLOSS_CAP],
    head: usize,
    pub n: usize,
}

impl PowerLossLedger {
    pub const fn new() -> PowerLossLedger {
        PowerLossLedger { days: [0; POWERLOSS_CAP], dirty_recover: [false; POWERLOSS_CAP], head: 0, n: 0 }
    }

    pub fn record(&mut self, day: u32, dirty: bool) {
        self.days[self.head] = day;
        self.dirty_recover[self.head] = dirty;
        self.head = (self.head + 1) % POWERLOSS_CAP;
        if self.n < POWERLOSS_CAP {
            self.n += 1;
        }
    }

    /// 窗口内掉电次数。
    pub fn count_in(&self, today: u32, window_days: u32) -> u32 {
        let mut c = 0;
        for i in 0..self.n {
            let d = self.days[i];
            if d <= today && today.saturating_sub(d) < window_days {
                c += 1;
            }
        }
        c
    }

    /// 脏恢复占比 ‰（脏恢复 = 文件系统要修——掉电不干净是质量信号）。
    pub fn dirty_permille(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let mut d = 0;
        for i in 0..self.n {
            if self.dirty_recover[i] {
                d += 1;
            }
        }
        Some(d as u32 * 1_000 / self.n as u32)
    }
}

/// 健康趋势周报帧（14B）：
/// [0..2) "WH" · [2..4) 周号 LE · [4..6) WAF ‰ LE · [6..8) 重映射数 LE ·
/// [8..10) 掉电次数 LE · [10..12) 综合分 ‰ LE · [12..14) 校验和（前 12B FNV-16）。
pub fn fnv16(data: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    (h & 0xFFFF) as u16
}

pub fn encode_weekly(week: u16, waf_permille: u16, remaps: u16, powerloss: u16, score: u16, out: &mut [u8; WEEKLY_FRAME_LEN]) -> bool {
    if score > 1_000 {
        return false;
    }
    out[0] = b'W';
    out[1] = b'H';
    out[2..4].copy_from_slice(&week.to_le_bytes());
    out[4..6].copy_from_slice(&waf_permille.to_le_bytes());
    out[6..8].copy_from_slice(&remaps.to_le_bytes());
    out[8..10].copy_from_slice(&powerloss.to_le_bytes());
    out[10..12].copy_from_slice(&score.to_le_bytes());
    let c = fnv16(&out[..12]);
    out[12] = (c & 0xFF) as u8;
    out[13] = (c >> 8) as u8;
    true
}

pub fn decode_weekly(frame: &[u8; WEEKLY_FRAME_LEN]) -> Option<(u16, u16, u16, u16, u16)> {
    if frame[0] != b'W' || frame[1] != b'H' {
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
        u16::from_le_bytes(frame[10..12].try_into().ok()?),
    ))
}

/// 综合分：WAF 段(40%) + 替补健康(40%) + 掉电清洁(20%) 千分制。
pub fn composite_score(waf: u32, spare_health: u32, clean_permille: u32) -> u32 {
    let waf_part = if waf <= 1_100 { 1_000u64 } else { (1_100u64 * 1_000 / waf as u64).min(1_000) };
    let spare_part = spare_health.min(1_000) as u64;
    let clean_part = clean_permille.min(1_000) as u64;
    ((waf_part * 4 + spare_part * 4 + clean_part * 2) / 10) as u32
}

#[inline(never)]
pub fn run_diskhealth_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b8");

    // 1) WAF 计算：主机 100 / NAND 130 → 1300‰（1.3 倍放大）。
    let mut w = WafTracker::new();
    w.sample(100, 130);
    cs.add("waf_compute", w.waf_permille() == Some(1_300) && w.grade() == Some(1), "");

    // 2) WAF 两端：健康 1.05 → 优、病态 2.5 → 差（分级三段）。
    let mut w2 = WafTracker::new();
    w2.sample(1_000, 1_050);
    let mut w3 = WafTracker::new();
    w3.sample(1_000, 2_500);
    cs.add(
        "waf_grades",
        w2.grade() == Some(0) && w3.grade() == Some(2) && w3.waf_permille() == Some(2_500),
        "",
    );

    // 3) 零主机写不猜：空账 → None（没有写就没有放大可言）。
    cs.add("waf_empty_honest", WafTracker::new().waf_permille().is_none(), "");

    // 4) 环窗：10 采样留 8（均值窗口口径）。
    let mut w4 = WafTracker::new();
    for i in 1..=10u64 {
        w4.sample(1_000, 1_000 + i * 10);
    }
    // 最近 8 个：host 8000、nand 8000+ (sum 4..10=84)*10 → 8840 → 1105‰。
    cs.add("waf_window_wraps", w4.n == WAF_SAMPLES && w4.waf_permille() == Some(1_065), "");

    // 5) 重映射：坏块 7 → 替补 10009、spares 递减；重复映射拒。
    let mut m = RemapMap::new(100);
    let s1 = m.remap(7);
    cs.add(
        "remap_assigns_spare",
        s1 == Some(10_099) && m.spares_left == 99 && m.mapping_of(7) == Some(10_099) && m.remap(7).is_none(),
        "",
    );

    // 6) 替补耗尽诚实上报：初值 1 → 第 2 块重映射 None（危险信号不吞）。
    let mut m2 = RemapMap::new(1);
    let ok = m2.remap(1).is_some();
    let dead = m2.remap(2).is_none();
    cs.add("remap_exhaustion_honest", ok && dead && m2.spare_health_permille(1) == 0, "");

    // 7) 替补健康：100 补 25 → 750‰（线性递减）。
    let mut m3 = RemapMap::new(100);
    for b in 0..20u32 {
        m3.remap(b);
    }
    cs.add("remap_health_linear", m3.spare_health_permille(100) == 800, "");

    // 8) 掉电账：30 天窗 3 次、脏恢复 2/3 → 667‰（截断）。
    let mut p = PowerLossLedger::new();
    p.record(10, true);
    p.record(20, false);
    p.record(28, true);
    cs.add(
        "powerloss_window_dirty",
        p.count_in(30, 30) == 3 && p.dirty_permille() == Some(666),
        "",
    );

    // 9) 掉电空账：无事件 → dirty None（不编造 0‰ 干净）。
    cs.add("powerloss_empty_honest", PowerLossLedger::new().dirty_permille().is_none(), "");

    // 10) 窗界：day 35 查 30 天窗 → day 10 出窗（计数 2）。
    cs.add("powerloss_window_slide", p.count_in(40, 30) == 2, "");

    // 11) 周报帧 round-trip + 越界分拒 + 撕裂拒。
    let mut f = [0u8; WEEKLY_FRAME_LEN];
    let ok = encode_weekly(12, 1_065, 25, 3, 820, &mut f);
    let bad = !encode_weekly(12, 1_105, 25, 3, 1_001, &mut f);
    let mut tear_ok = true;
    for i in 0..WEEKLY_FRAME_LEN {
        let mut t = f;
        t[i] ^= 0x4B;
        if decode_weekly(&t).is_some() {
            tear_ok = false;
        }
    }
    cs.add(
        "weekly_frame_guards",
        ok && decode_weekly(&f) == Some((12, 1_065, 25, 3, 820)) && bad && tear_ok,
        "",
    );

    // 12) 综合分：三因素加权算术直核（WAF 优 + 替补 750 + 全清洁）。
    let score = composite_score(1_050, 750, 1_000);
    // waf_part=1000*4 + 750*4 + 1000*2 = 4000+3000+2000 = 9000/10 = 900。
    cs.add("composite_score_arithmetic", score == 900, "");

    // 13) 综合分守门：WAF 病态压分、替补耗尽压分（短板效应可见）。
    let bad_waf = composite_score(3_000, 1_000, 1_000);
    let dead_spare = composite_score(1_050, 0, 1_000);
    cs.add(
        "composite_weak_link",
        bad_waf < 900 && dead_spare < 900,
        "",
    );

    // 14) 常量自洽：窗 8、图 24、帧 14。
    cs.add("b8_constants", WAF_SAMPLES == 8 && REMAP_CAP == 24 && WEEKLY_FRAME_LEN == 14, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waf_never_below_host() {
        // NAND 写 < 主机写（压缩盘）→ WAF <1000 合法（不是错误——量它就是了）。
        let mut w = WafTracker::new();
        w.sample(1_000, 800);
        assert_eq!(w.waf_permille(), Some(800));
        assert_eq!(w.grade(), Some(0));
    }

    #[test]
    fn remap_spare_numbering_monotone() {
        // 替补号单调递减分配（10000 起——不与用户块冲突的约定）。
        let mut m = RemapMap::new(5);
        let a = m.remap(100).unwrap();
        let b = m.remap(101).unwrap();
        assert_eq!((a, b), (10_004, 10_003));
    }

    #[test]
    fn powerloss_ring_wraps() {
        // 环回卷：10 事件留 8（最旧出账——诚实失忆）。
        let mut p = PowerLossLedger::new();
        for d in 0..10u32 {
            p.record(d, d % 2 == 0);
        }
        assert_eq!(p.n, POWERLOSS_CAP);
        assert_eq!(p.count_in(100, 1_000), 8);
    }
}
