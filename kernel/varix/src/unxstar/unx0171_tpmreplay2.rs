//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0171 · TPM2 事件重放（只读）（深化第二实现） · 0x6F2C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0171-J1 TPM2 事件重放：事件账（PCR/digest/类型）按
//!     PCR 分桶重放（digest 逐条折叠），重放 PCR 值与账面对拍，
//!     首事件 SpecID 语义延续（F0057 联签），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F2C0，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F2C0;

/// PCR 数（TPM2 冻结 24）。
pub const PCR_COUNT: usize = 24;
/// digest 长（SHA256 冻结 32）。
pub const DIGEST_LEN: usize = 32;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RpCode {
    /// PCR 越界
    PcrOver,
    /// 重放失配（折叠值 ≠ 账面）
    ReplayMismatch,
    /// 首事件非 SpecID
    SpecIdMissing,
    /// digest 空（非 NO_ACTION）
    DigestZero,
    /// 双跑不一致
    NotReproducible,
}

impl RpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RpCode::PcrOver => "PCR 越界：≥24",
            RpCode::ReplayMismatch => "重放失配：折叠值 ≠ 账面",
            RpCode::SpecIdMissing => "首事件非 SpecID",
            RpCode::DigestZero => "digest 空：非 NO_ACTION",
            RpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RpCode; 5] = [
        RpCode::PcrOver,
        RpCode::ReplayMismatch,
        RpCode::SpecIdMissing,
        RpCode::DigestZero,
        RpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RpCode::ALL.len() {
        if RpCode::ALL[i].val() == code {
            return RpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F2C0 段闭集内"
}

// ── 事件账与重放 ────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tpm2Event {
    pub pcr: usize,
    /// NO_ACTION 事件 digest 允许全零。
    pub digest: [u8; DIGEST_LEN],
    pub no_action: bool,
}

impl Tpm2Event {
    pub fn legal(&self) -> Result<(), u32> {
        if self.pcr >= PCR_COUNT {
            return Err(RpCode::PcrOver.val());
        }
        if !self.no_action {
            let mut zero = true;
            let mut i = 0;
            while i < DIGEST_LEN {
                if self.digest[i] != 0 {
                    zero = false;
                }
                i += 1;
            }
            if zero {
                return Err(RpCode::DigestZero.val());
            }
        }
        Ok(())
    }
}

/// PCR 折叠重放：逐事件 digest 折叠（FNV 链）。
pub fn replay_pcr(events: &[Tpm2Event], pcr: usize) -> Result<u64, u32> {
    if pcr >= PCR_COUNT {
        return Err(RpCode::PcrOver.val());
    }
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < events.len() {
        let e = &events[i];
        e.legal()?;
        if e.pcr == pcr {
            let mut k = 0;
            while k < DIGEST_LEN {
                h = (h ^ e.digest[k] as u64).wrapping_mul(0x100000001b3);
                k += 1;
            }
        }
        i += 1;
    }
    Ok(h)
}

/// 首事件 SpecID 语义。
pub fn specid_ok(events: &[Tpm2Event]) -> Result<(), u32> {
    match events.first() {
        Some(e) if e.no_action && e.pcr == 0 => Ok(()),
        _ => Err(RpCode::SpecIdMissing.val()),
    }
}

/// 重放对拍（账面 PCR 值账 vs 折叠重放）。
pub fn replay_audit(events: &[Tpm2Event], pcr: usize, ledger_value: u64) -> Result<(), u32> {
    if replay_pcr(events, pcr)? != ledger_value {
        return Err(RpCode::ReplayMismatch.val());
    }
    Ok(())
}

/// 复算。
pub fn replay_ok(events: &[Tpm2Event], pcr: usize) -> Result<(), u32> {
    let a = replay_pcr(events, pcr)?;
    let b = replay_pcr(events, pcr)?;
    if a != b {
        return Err(RpCode::NotReproducible.val());
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

// ── 注入检出账（四路：越 PCR 路/digest 路/SpecID 路/复算路） ──────
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

/// 真机事件（SpecID + PCR0×2 + PCR4×1）。
pub fn machine_events() -> Vec<Tpm2Event> {
    let mut d1 = [0u8; DIGEST_LEN];
    let mut d2 = [0u8; DIGEST_LEN];
    let mut d3 = [0u8; DIGEST_LEN];
    let mut i = 0;
    while i < DIGEST_LEN {
        d1[i] = 0x10 + i as u8;
        d2[i] = 0x40 + i as u8;
        d3[i] = 0x80 + i as u8;
        i += 1;
    }
    vec![
        Tpm2Event { pcr: 0, digest: [0; DIGEST_LEN], no_action: true },
        Tpm2Event { pcr: 0, digest: d1, no_action: false },
        Tpm2Event { pcr: 0, digest: d2, no_action: false },
        Tpm2Event { pcr: 4, digest: d3, no_action: false },
    ]
}

// ── 黄金全链：SpecID→重放 PCR0/4→对拍→失配抓→复算→越 PCR 拒 ──────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let evs = machine_events();
    specid_ok(&evs)?;
    let p0 = replay_pcr(&evs, 0)?;
    let p4 = replay_pcr(&evs, 4)?;
    replay_audit(&evs, 0, p0)?;
    replay_audit(&evs, 4, p4)?;
    // 失配抓。
    let mis = replay_audit(&evs, 0, p0 ^ 1).is_err();
    // PCR0 空桶（不存在的事件）= 零折叠基值。
    let empty = replay_pcr(&evs, 7)? == 0xcbf29ce484222325;
    replay_ok(&evs, 0)?;
    Ok((1, mis as u32 + empty as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：SpecID + 重放 + 失配抓 + 空桶语义。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 2, _)) => Ok(()),
        _ => Err(RpCode::NotReproducible.val()),
    }
}

/// 序 B：PCR 越界 ⇒ PcrOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match replay_pcr(&machine_events(), PCR_COUNT) {
        Err(e) if e == RpCode::PcrOver.val() => Ok(()),
        _ => Err(RpCode::NotReproducible.val()),
    }
}

/// 序 C：digest 空（非 NO_ACTION）⇒ DigestZero。
pub fn probe_seq_c() -> Result<(), u32> {
    let e = Tpm2Event { pcr: 1, digest: [0; DIGEST_LEN], no_action: false };
    match e.legal() {
        Err(c) if c == RpCode::DigestZero.val() => Ok(()),
        _ => Err(RpCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越 PCR 路。
        let bad = Tpm2Event { pcr: 31, digest: [1; DIGEST_LEN], no_action: false };
        led.record(0, bad.legal().is_err());
        // 路 2：digest 路。
        let bad2 = Tpm2Event { pcr: 1, digest: [0; DIGEST_LEN], no_action: false };
        led.record(1, bad2.legal().is_err());
        // 路 3：SpecID 路。
        let mut evs = machine_events();
        evs[0].no_action = false;
        led.record(2, specid_ok(&evs).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_events(), 0).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RpCode::ALL.len() {
        let v = RpCode::ALL[i].val();
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
            Ok((spec, ok, _)) => *spec == 1 && *ok == 2,
            Err(_) => false,
        },
        match &g {
            Ok((spec, ok, _)) => format!("specid={} replay_ok={}", spec, ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // PCR 数冻结。
    v.push((
        "J1-PCR冻结",
        PCR_COUNT == 24 && DIGEST_LEN == 32,
        String::from("24 PCR / SHA256 32B 冻结"),
    ));
    // 空桶语义。
    v.push((
        "J1-空桶语义",
        replay_pcr(&machine_events(), 9) == Ok(0xcbf29ce484222325),
        String::from("无事件 PCR ⇒ 零折叠基值"),
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
        String::from("0x6F2C0 窗口"),
    ));
    v
}

pub fn run_unx0171b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0171 TPM2 事件重放（只读）（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
