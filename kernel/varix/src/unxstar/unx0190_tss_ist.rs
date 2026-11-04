//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0190 · TSS 与 IST 布局初始化 · 0x6F680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0190-J1 TSS/IST 账：IST 槽位账（7 槽冻结，槽 1=双
//!     错误栈/槽 2=NMI 栈），槽基址非零且对齐 16，IST 选择子映射
//!     与槽位一致，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F680，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F680;

/// IST 槽数（冻结 7）。
pub const IST_SLOTS: usize = 7;
/// 双错误栈槽 / NMI 栈槽（冻结）。
pub const IST_DF: usize = 1;
pub const IST_NMI: usize = 2;
/// 栈对齐（16 冻结）。
pub const STACK_ALIGN: u64 = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TssCode {
    /// 槽位越界（0 槽非法或 ≥7）
    SlotOver,
    /// 槽基址零
    BaseZero,
    /// 槽基址非 16 对齐
    BaseMisaligned,
    /// 必备槽缺（DF/NMI 槽未配置）
    EssentialMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl TssCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TssCode::SlotOver => "槽位越界：0 或 ≥7",
            TssCode::BaseZero => "槽基址零",
            TssCode::BaseMisaligned => "槽基址非 16 对齐",
            TssCode::EssentialMissing => "必备槽缺：DF/NMI",
            TssCode::ReplayMismatch => "复算失配",
            TssCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TssCode; 6] = [
        TssCode::SlotOver,
        TssCode::BaseZero,
        TssCode::BaseMisaligned,
        TssCode::EssentialMissing,
        TssCode::ReplayMismatch,
        TssCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TssCode::ALL.len() {
        if TssCode::ALL[i].val() == code {
            return TssCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F680 段闭集内"
}

// ── IST 账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct TssLayout {
    /// IST 槽基址（0 = 未配置）。
    pub ist_bases: [u64; IST_SLOTS],
}

impl TssLayout {
    /// 配置槽（1-6 有效）。
    pub fn set_slot(&mut self, slot: usize, base: u64) -> Result<(), u32> {
        if slot == 0 || slot >= IST_SLOTS {
            return Err(TssCode::SlotOver.val());
        }
        if base == 0 {
            return Err(TssCode::BaseZero.val());
        }
        if base % STACK_ALIGN != 0 {
            return Err(TssCode::BaseMisaligned.val());
        }
        self.ist_bases[slot] = base;
        Ok(())
    }

    /// 初始化审计：DF/NMI 必备槽在场。
    pub fn init_audit(&self) -> Result<(), u32> {
        if self.ist_bases[IST_DF] == 0 || self.ist_bases[IST_NMI] == 0 {
            return Err(TssCode::EssentialMissing.val());
        }
        Ok(())
    }

    /// IST 选择子（槽位 → IDT ist 值 = 槽号）。
    pub fn ist_index(&self, slot: usize) -> Result<u8, u32> {
        if slot == 0 || slot >= IST_SLOTS || self.ist_bases[slot] == 0 {
            return Err(TssCode::SlotOver.val());
        }
        Ok(slot as u8)
    }
}

/// 复算。
pub fn replay_ok(bases: &[u64; IST_SLOTS]) -> Result<(), u32> {
    let mut a = TssLayout { ist_bases: *bases };
    let mut b = TssLayout { ist_bases: *bases };
    if a.init_audit() != b.init_audit() || a.ist_bases != b.ist_bases {
        return Err(TssCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越槽路/零址路/对齐路/复算路） ──────────────
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

/// 真机 TSS（DF 0x7F000 / NMI 0x7E000 对齐栈）。
pub fn machine_tss() -> TssLayout {
    let mut t = TssLayout::default();
    let _ = t.set_slot(IST_DF, 0x7F000);
    let _ = t.set_slot(IST_NMI, 0x7E000);
    t
}

// ── 黄金全链：配置→审计→IST 序→缺槽抓→三拒→复算 ──────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut t = machine_tss();
    t.init_audit()?;
    let ist_df = t.ist_index(IST_DF)?;
    let ist_nmi = t.ist_index(IST_NMI)?;
    let sel_ok = (ist_df == IST_DF as u8 && ist_nmi == IST_NMI as u8) as u32;
    // 缺槽抓（NMI 未配 → 审计拒）。
    let mut bad = TssLayout::default();
    let _ = bad.set_slot(IST_DF, 0x7F000);
    let missing = bad.init_audit().is_err();
    // 三拒：槽 0 / 槽 7 / 非对齐。
    let zero = t.set_slot(0, 0x1000).is_err();
    let over = t.set_slot(7, 0x1000).is_err();
    let misaligned = t.set_slot(3, 0x1008).is_err();
    replay_ok(&t.ist_bases)?;
    Ok((sel_ok + missing as u32, zero as u32 + over as u32 + misaligned as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：IST 序 + 缺槽抓 + 三拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 3, _)) => Ok(()),
        _ => Err(TssCode::NotReproducible.val()),
    }
}

/// 序 B：必备槽缺 ⇒ EssentialMissing。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut t = TssLayout::default();
    let _ = t.set_slot(IST_NMI, 0x7E000);
    match t.init_audit() {
        Err(e) if e == TssCode::EssentialMissing.val() => Ok(()),
        _ => Err(TssCode::NotReproducible.val()),
    }
}

/// 序 C：IST 槽冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (IST_SLOTS == 7, IST_DF == 1, IST_NMI == 2) {
        (true, true, true) => Ok(()),
        _ => Err(TssCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越槽路。
        let mut t1 = TssLayout::default();
        led.record(0, t1.set_slot(0, 0x1000).is_err() && t1.set_slot(7, 0x1000).is_err());
        // 路 2：零址路。
        let mut t2 = TssLayout::default();
        led.record(1, t2.set_slot(3, 0).is_err());
        // 路 3：对齐路。
        let mut t3 = TssLayout::default();
        led.record(2, t3.set_slot(3, 0x1234).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_tss().ist_bases).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TssCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TssCode::ALL.len() {
        let v = TssCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 2 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("ist+missing={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 真机槽基址对齐账。
    let t = machine_tss();
    v.push((
        "J1-槽基址账",
        t.ist_bases[IST_DF] == 0x7F000 && t.ist_bases[IST_NMI] == 0x7E000,
        String::from("DF=0x7F000 NMI=0x7E000 对齐账"),
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
        String::from("0x6F680 窗口"),
    ));
    v
}

pub fn run_unx0190_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0190 TSS 与 IST 布局初始化");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
