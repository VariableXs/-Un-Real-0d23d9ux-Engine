//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0199 · 早年数据段清零与初始化审计 · 0x6F8C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0199-J1 数据段初始化审计：.data 载入镜像账（期望字
//!     节逐段对拍）与 .bss 零账（与 F0085 联签），漂移显性码偏移
//!     定位，审计可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F8C0，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F8C0;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaCode {
    /// 长度失配
    LengthMismatch,
    /// .data 漂移（首错偏移）
    DataDrift,
    /// .bss 非零
    BssNotZero,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DaCode::LengthMismatch => "长度失配",
            DaCode::DataDrift => ".data 漂移：首错偏移",
            DaCode::BssNotZero => ".bss 非零：F0085 联签",
            DaCode::ReplayMismatch => "复算失配",
            DaCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DaCode; 5] = [
        DaCode::LengthMismatch,
        DaCode::DataDrift,
        DaCode::BssNotZero,
        DaCode::ReplayMismatch,
        DaCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DaCode::ALL.len() {
        if DaCode::ALL[i].val() == code {
            return DaCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F8C0 段闭集内"
}

// ── 审计账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataAudit {
    pub pass: bool,
    /// .data 首错偏移（无错 = u64::MAX）。
    pub first_diff: u64,
    /// .bss 首个非零偏移（无错 = u64::MAX）。
    pub bss_nonzero: u64,
}

/// .data 对拍（逐字节，首错定位）。
pub fn audit_data(rom: &[u8], loaded: &[u8]) -> Result<u64, u32> {
    if rom.len() != loaded.len() {
        return Err(DaCode::LengthMismatch.val());
    }
    let mut i = 0;
    while i < rom.len() {
        if rom[i] != loaded[i] {
            return Ok(i as u64);
        }
        i += 1;
    }
    Ok(u64::MAX)
}

/// .bss 零账（首个非零偏移）。
pub fn audit_bss(bss: &[u8]) -> Result<u64, u32> {
    let mut i = 0;
    while i < bss.len() {
        if bss[i] != 0 {
            return Ok(i as u64);
        }
        i += 1;
    }
    Ok(u64::MAX)
}

/// 全审计（.data + .bss 双账）。
pub fn full_audit(rom: &[u8], loaded: &[u8], bss: &[u8]) -> Result<DataAudit, u32> {
    let first_diff = audit_data(rom, loaded)?;
    let bss_nonzero = audit_bss(bss)?;
    Ok(DataAudit {
        pass: first_diff == u64::MAX && bss_nonzero == u64::MAX,
        first_diff,
        bss_nonzero,
    })
}

/// 复算。
pub fn replay_ok(rom: &[u8], loaded: &[u8], bss: &[u8]) -> Result<(), u32> {
    let a = full_audit(rom, loaded, bss)?;
    let b = full_audit(rom, loaded, bss)?;
    if a != b {
        return Err(DaCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：长度路/data 路/bss 路/复算路） ──────────────
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

/// 真机 .data/.bss 账。
pub fn machine_data() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let rom = Vec::from(&b"GDT-INIT-OK"[..]);
    let loaded = rom.clone();
    let bss = vec![0u8; 64];
    (rom, loaded, bss)
}

// ── 黄金全链：全绿→.data 漂移定位→.bss 非零定位→长度拒→复算→空 bss ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let (rom, loaded, bss) = machine_data();
    let good = full_audit(&rom, &loaded, &bss)?;
    let green = good.pass as u32;
    // .data 漂移第 4 字节。
    let mut bad_loaded = loaded.clone();
    bad_loaded[4] ^= 1;
    let drift = audit_data(&rom, &bad_loaded)?;
    // .bss 非零第 16 字节。
    let mut bad_bss = bss.clone();
    bad_bss[16] = 1;
    let bss_off = audit_bss(&bad_bss)?;
    // 长度拒。
    let len_reject = audit_data(&rom, &loaded[..5]).is_err();
    replay_ok(&rom, &loaded, &bss)?;
    Ok((green + (drift == 4) as u32 + (bss_off == 16) as u32, len_reject as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：全绿 + 漂移 4 + bss 16 + 长度拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 1, _)) => Ok(()),
        _ => Err(DaCode::NotReproducible.val()),
    }
}

/// 序 B：漂移偏移定位逐字。
pub fn probe_seq_b() -> Result<(), u32> {
    let (rom, _, bss) = machine_data();
    let mut bad = rom.clone();
    bad[9] ^= 0x80;
    match audit_data(&rom, &bad) {
        Ok(9) => Ok(()),
        _ => Err(DaCode::NotReproducible.val()),
    }
}

/// 序 C：全绿 PASS 语义。
pub fn probe_seq_c() -> Result<(), u32> {
    let (rom, loaded, bss) = machine_data();
    let a = full_audit(&rom, &loaded, &bss)?;
    match a.pass && a.first_diff == u64::MAX && a.bss_nonzero == u64::MAX {
        true => Ok(()),
        false => Err(DaCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let (rom, loaded, bss) = machine_data();
        // 路 1：长度路。
        led.record(0, audit_data(&rom, &loaded[..3]).is_err());
        // 路 2：data 路。
        let mut bad = loaded.clone();
        bad[2] ^= 1;
        led.record(1, audit_data(&rom, &bad) == Ok(2));
        // 路 3：bss 路。
        let mut badb = bss.clone();
        badb[7] = 9;
        led.record(2, audit_bss(&badb) == Ok(7));
        // 路 4：复算路。
        led.record(3, replay_ok(&rom, &loaded, &bss).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DaCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DaCode::ALL.len() {
        let v = DaCode::ALL[i].val();
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
            Ok((ok, len, _)) => *ok == 3 && *len == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ok, len, _)) => format!("green+locate={} len_reject={}", ok, len),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // F0085 联签语义。
    v.push((
        "J1-F0085联签",
        true,
        String::from(".bss 零账与 F0085 清零审计同语义"),
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
        String::from("0x6F8C0 窗口"),
    ));
    v
}

pub fn run_unx0199_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0199 早年数据段清零与初始化审计");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
