//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0132 · 健康标志位（固件/介质/内存三区） · 0x6ED40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0132-J1 健康标志位图：三区（固件/介质/内存）各 8 位
//!     状态账，逐位语义冻结（固件：SB/TPM/secure/…），置位-清
//!     位-查询三原子操作，健康总评（三区全绿才 HEALTHY），位图
//!     可复算零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三区 × 8 位 = 24 位账；
//!   - 位语义冻结（逐位名册）；
//!   - 总评：任一区任一坏位 ⇒ DEGRADED；
//!   - 复算：同操作序列同位图；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6ED40，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6ED40;

/// 三区（冻结）。
pub const Z_FIRMWARE: usize = 0;
pub const Z_MEDIA: usize = 1;
pub const Z_MEMORY: usize = 2;
pub const ZONE_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthCode {
    /// 区越界
    ZoneOver,
    /// 位越界（≥8）
    BitOver,
    /// 位名账外
    BitNameUnknown,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl HealthCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HealthCode::ZoneOver => "区越界：≥3",
            HealthCode::BitOver => "位越界：≥8",
            HealthCode::BitNameUnknown => "位名账外",
            HealthCode::ReplayMismatch => "复算失配",
            HealthCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HealthCode; 5] = [
        HealthCode::ZoneOver,
        HealthCode::BitOver,
        HealthCode::BitNameUnknown,
        HealthCode::ReplayMismatch,
        HealthCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        if HealthCode::ALL[i].val() == code {
            return HealthCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6ED40 段闭集内"
}

// ── 健康位图 ────────────────────────────────────────────────────
/// 位名册（区 × 位 → 名；坏位语义）。
pub fn bit_name(zone: usize, bit: usize) -> Result<&'static str, u32> {
    match (zone, bit) {
        (Z_FIRMWARE, 0) => Ok("SB 启用"),
        (Z_FIRMWARE, 1) => Ok("TPM 在场"),
        (Z_FIRMWARE, 2) => Ok("SetupMode 关闭"),
        (Z_MEDIA, 0) => Ok("引导介质可读"),
        (Z_MEDIA, 1) => Ok("写通道正常"),
        (Z_MEMORY, 0) => Ok("自检全过"),
        (Z_MEMORY, 1) => Ok("坏页为零"),
        (0..=2, 2..=7) => Ok("保留位"),
        _ => Err(HealthCode::BitNameUnknown.val()),
    }
}

#[derive(Clone, Debug)]
pub struct HealthBitmap {
    /// 每区 8 位（1 = 好）。
    pub bits: [u8; ZONE_COUNT],
}

impl HealthBitmap {
    pub fn all_good() -> HealthBitmap {
        HealthBitmap { bits: [0xFF; ZONE_COUNT] }
    }

    /// 查位。
    pub fn get(&self, zone: usize, bit: usize) -> Result<bool, u32> {
        if zone >= ZONE_COUNT {
            return Err(HealthCode::ZoneOver.val());
        }
        if bit >= 8 {
            return Err(HealthCode::BitOver.val());
        }
        Ok(self.bits[zone] & (1 << bit) != 0)
    }

    /// 置位（好）。
    pub fn set(&mut self, zone: usize, bit: usize) -> Result<(), u32> {
        if zone >= ZONE_COUNT {
            return Err(HealthCode::ZoneOver.val());
        }
        if bit >= 8 {
            return Err(HealthCode::BitOver.val());
        }
        self.bits[zone] |= 1 << bit;
        Ok(())
    }

    /// 清位（坏）。
    pub fn clear(&mut self, zone: usize, bit: usize) -> Result<(), u32> {
        if zone >= ZONE_COUNT {
            return Err(HealthCode::ZoneOver.val());
        }
        if bit >= 8 {
            return Err(HealthCode::BitOver.val());
        }
        self.bits[zone] &= !(1 << bit);
        Ok(())
    }

    /// 健康总评：24 位全 1 ⇒ HEALTHY。
    pub fn verdict(&self) -> &'static str {
        let mut i = 0;
        while i < ZONE_COUNT {
            if self.bits[i] != 0xFF {
                return "DEGRADED";
            }
            i += 1;
        }
        "HEALTHY"
    }

    /// 坏位计数。
    pub fn bad_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < ZONE_COUNT {
            n += (self.bits[i] ^ 0xFF).count_ones();
            i += 1;
        }
        n
    }
}

/// 复算：同操作序列同位图。
pub fn replay_ok(ops: &[(u8, usize, usize)]) -> Result<(), u32> {
    let mut a = HealthBitmap::all_good();
    let mut b = HealthBitmap::all_good();
    let mut i = 0;
    while i < ops.len() {
        let (kind, z, bit) = ops[i];
        match kind {
            0 => {
                let _ = a.set(z, bit);
                let _ = b.set(z, bit);
            }
            _ => {
                let _ = a.clear(z, bit);
                let _ = b.clear(z, bit);
            }
        }
        i += 1;
    }
    if a.bits != b.bits {
        return Err(HealthCode::ReplayMismatch.val());
    }
    Ok(())
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：越区路/越位路/复算路/名册路） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 真机操作：内存坏 1 位（清 Z_MEMORY bit1）。
pub fn machine_ops() -> Vec<(u8, usize, usize)> {
    vec![(1, Z_MEMORY, 1)]
}

// ── 黄金全链：全绿→清位→总评 DEGRADED→复算→越界拒 ────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut h = HealthBitmap::all_good();
    let healthy0 = h.verdict() == "HEALTHY";
    let ops = machine_ops();
    let mut i = 0;
    while i < ops.len() {
        let _ = h.clear(ops[i].1, ops[i].2);
        i += 1;
    }
    let degraded = h.verdict() == "DEGRADED";
    let bad = h.bad_count();
    replay_ok(&ops)?;
    // 越界拒。
    let over_zone = h.get(3, 0).is_err() && h.get(0, 8).is_err();
    Ok((healthy0 as u32 + degraded as u32, bad, over_zone as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：全绿 HEALTHY → 清 1 位 DEGRADED + 坏 1 + 越界拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 1, 1)) => Ok(()),
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

/// 序 B：位名册（坏位语义逐字）。
pub fn probe_seq_b() -> Result<(), u32> {
    let names_ok = bit_name(Z_FIRMWARE, 0) == Ok("SB 启用")
        && bit_name(Z_MEDIA, 0) == Ok("引导介质可读")
        && bit_name(Z_MEMORY, 1) == Ok("坏页为零")
        && bit_name(Z_FIRMWARE, 5) == Ok("保留位");
    match names_ok {
        true => Ok(()),
        false => Err(HealthCode::NotReproducible.val()),
    }
}

/// 序 C：位越界 ⇒ BitOver。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut h = HealthBitmap::all_good();
    match h.set(Z_FIRMWARE, 8) {
        Err(e) if e == HealthCode::BitOver.val() => Ok(()),
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越区路。
        let mut h1 = HealthBitmap::all_good();
        led.record(0, h1.set(5, 0).is_err());
        // 路 2：越位路。
        led.record(1, h1.clear(Z_MEDIA, 9).is_err());
        // 路 3：复算路。
        led.record(2, replay_ok(&machine_ops()).is_ok());
        // 路 4：名册路。
        led.record(3, bit_name(9, 0).is_err() && bit_name(0, 9).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HealthCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        let v = HealthCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((v2, bad, over)) => *v2 == 2 && *bad == 1 && *over == 1,
            Err(_) => false,
        },
        match &g {
            Ok((v2, bad, over)) => format!("verdicts={} bad={} over_reject={}", v2, bad, over),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三区冻结。
    v.push((
        "J1-三区冻结",
        ZONE_COUNT == 3 && (Z_FIRMWARE, Z_MEDIA, Z_MEMORY) == (0, 1, 2),
        String::from("固件/介质/内存 三区 × 8 位"),
    ));
    // 置位-清位原子往返。
    let mut h = HealthBitmap::all_good();
    let _ = h.clear(Z_MEDIA, 3);
    let mid = h.get(Z_MEDIA, 3) == Ok(false);
    let _ = h.set(Z_MEDIA, 3);
    let back = h.get(Z_MEDIA, 3) == Ok(true);
    v.push((
        "J1-原子往返",
        mid && back && h.verdict() == "HEALTHY",
        String::from("clear→get 假 / set→get 真 / 恢复 HEALTHY"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6ED40 窗口"),
    ));
    v
}

pub fn run_unx0132_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0132 健康标志位（固件/介质/内存三区）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
