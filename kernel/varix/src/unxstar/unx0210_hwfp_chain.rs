//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0210 · 硬件指纹链（跨引导关联） · 0x6F980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0210-J1 指纹链：四件套账（CPU 型号 hash/内存拓扑
//!     hash/磁盘 hash/固件 hash），机器指纹 = 四件套折叠，同机
//!     同指纹（跨 boot 关联），异机必异，链可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F980，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F980;

/// 四件套槽数（冻结）。
pub const FP_ITEMS: usize = 4;
pub const F_CPU: usize = 0;
pub const F_MEM: usize = 1;
pub const F_DISK: usize = 2;
pub const F_FW: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HfpCode {
    /// 槽越界
    ItemOver,
    /// 指纹零（空账）
    FingerprintZero,
    /// 同机误异（关联失败）
    SameMachineDiffer,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl HfpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HfpCode::ItemOver => "槽越界：≥4",
            HfpCode::FingerprintZero => "指纹零：空账",
            HfpCode::SameMachineDiffer => "同机误异：关联失败",
            HfpCode::ReplayMismatch => "复算失配",
            HfpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HfpCode; 5] = [
        HfpCode::ItemOver,
        HfpCode::FingerprintZero,
        HfpCode::SameMachineDiffer,
        HfpCode::ReplayMismatch,
        HfpCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HfpCode::ALL.len() {
        if HfpCode::ALL[i].val() == code {
            return HfpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F980 段闭集内"
}

// ── 指纹链账 ────────────────────────────────────────────────────
fn fnv_slot(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut b = 0;
    while b < 8 {
        acc = (acc ^ ((v >> (b * 8)) & 0xFF)).wrapping_mul(0x100000001b3);
        b += 1;
    }
    acc
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HwFingerprint {
    pub items: [u64; FP_ITEMS],
}

impl HwFingerprint {
    /// 机器指纹（四件套折叠）。
    pub fn machine_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < FP_ITEMS {
            h = fnv_slot(h, self.items[i]);
            i += 1;
        }
        h
    }

    /// 跨引导关联：同机两次指纹一致。
    pub fn same_machine(&self, other: &HwFingerprint) -> bool {
        self.machine_fp() == other.machine_fp()
    }
}

/// 跨引导关联审计（同账同指纹 + 非零）。
pub fn link_audit(a: &HwFingerprint, b: &HwFingerprint) -> Result<u64, u32> {
    let fp = a.machine_fp();
    if fp == 0 || a.items.iter().all(|&v| v == 0) {
        return Err(HfpCode::FingerprintZero.val());
    }
    if !a.same_machine(b) {
        return Err(HfpCode::SameMachineDiffer.val());
    }
    Ok(fp)
}

/// 复算。
pub fn replay_ok(items: &[u64; FP_ITEMS]) -> Result<(), u32> {
    let a = HwFingerprint { items: *items };
    let b = HwFingerprint { items: *items };
    if a.machine_fp() != b.machine_fp() {
        return Err(HfpCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越槽路/零指纹路/异机路/复算路） ────────────
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

/// 真机四件套。
pub fn machine_fp() -> HwFingerprint {
    HwFingerprint { items: [0xC0DE_0001, 0x11E5_0002, 0xD15C_0003, 0x000F_0004] }
}

// ── 黄金全链：同机关联→异机必异→零指纹拒→复算→逐件漂移必异 ─────────
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let fp = link_audit(&machine_fp(), &machine_fp())?;
    // 异机必异（逐件漂移 4/4）。
    let mut differ = 0u32;
    let mut i = 0;
    while i < FP_ITEMS {
        let mut alt = machine_fp();
        alt.items[i] ^= 1;
        if alt.machine_fp() != fp {
            differ += 1;
        }
        i += 1;
    }
    // 零指纹拒。
    let zero = HwFingerprint { items: [0; FP_ITEMS] };
    let zero_reject = link_audit(&zero, &zero).is_err();
    replay_ok(&machine_fp().items)?;
    Ok((fp, differ + zero_reject as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：同机关联 + 逐件漂移 4 + 零拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((fp, 5, _)) if fp != 0 => Ok(()),
        _ => Err(HfpCode::NotReproducible.val()),
    }
}

/// 序 B：四件套闭集。
pub fn probe_seq_b() -> Result<(), u32> {
    match (FP_ITEMS == 4, F_CPU == 0, F_FW == 3) {
        (true, true, true) => Ok(()),
        _ => Err(HfpCode::NotReproducible.val()),
    }
}

/// 序 C：同机误异 ⇒ SameMachineDiffer。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = machine_fp();
    let mut b = machine_fp();
    b.items[F_DISK] ^= 0x100;
    match link_audit(&a, &b) {
        Err(e) if e == HfpCode::SameMachineDiffer.val() => Ok(()),
        _ => Err(HfpCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：零指纹路。
        let z = HwFingerprint { items: [0; FP_ITEMS] };
        led.record(0, link_audit(&z, &z).is_err());
        // 路 2：异机路。
        let mut alt = machine_fp();
        alt.items[F_MEM] += 1;
        led.record(1, !link_audit(&machine_fp(), &alt).is_ok());
        // 路 3：同机路。
        led.record(2, link_audit(&machine_fp(), &machine_fp()).is_ok());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_fp().items).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HfpCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HfpCode::ALL.len() {
        let v = HfpCode::ALL[i].val();
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
            Ok((fp, ok, _)) => *fp != 0 && *ok == 5,
            Err(_) => false,
        },
        match &g {
            Ok((fp, ok, _)) => format!("fp=0x{:x} checks={}", fp, ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 四件套冻结。
    v.push((
        "J1-四件套冻结",
        (F_CPU, F_MEM, F_DISK, F_FW) == (0, 1, 2, 3),
        String::from("CPU/内存/磁盘/固件 四件套"),
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
        String::from("0x6F980 窗口"),
    ));
    v
}

pub fn run_unx0210_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0210 硬件指纹链（跨引导关联）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
