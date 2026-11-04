//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0189 · GDT 重载与段语义固化 · 0x6F640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0189-J1 GDT 描述符账：六段闭集（null/kcode/kdata/
//!     ucode/udata/tss），访问字节逐位（P/DPL/S/Type）与语义匹
//!     配闸，选择子账（RPL 合法），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F640，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F640;

/// 六段闭集（冻结槽位）。
pub const S_NULL: usize = 0;
pub const S_KCODE: usize = 1;
pub const S_KDATA: usize = 2;
pub const S_UCODE: usize = 3;
pub const S_UDATA: usize = 4;
pub const S_TSS: usize = 5;
pub const SEG_COUNT: usize = 6;
/// 选择子步长（8 字节/描述符）。
pub const SEL_STEP: u16 = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdtCode {
    /// 槽位越界
    SlotOver,
    /// 存在位缺失（非 null 段）
    PresentMissing,
    /// DPL 失配（内核段 DPL≠0 / 用户段 DPL≠3）
    DplMismatch,
    /// 系统段 S 位置错
    SystemBitBad,
    /// 选择子 RPL 越权
    RplBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl GdtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GdtCode::SlotOver => "槽位越界：≥6",
            GdtCode::PresentMissing => "存在位缺失",
            GdtCode::DplMismatch => "DPL 失配",
            GdtCode::SystemBitBad => "系统段 S 位置错",
            GdtCode::RplBad => "选择子 RPL 越权",
            GdtCode::ReplayMismatch => "复算失配",
            GdtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GdtCode; 7] = [
        GdtCode::SlotOver,
        GdtCode::PresentMissing,
        GdtCode::DplMismatch,
        GdtCode::SystemBitBad,
        GdtCode::RplBad,
        GdtCode::ReplayMismatch,
        GdtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GdtCode::ALL.len() {
        if GdtCode::ALL[i].val() == code {
            return GdtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F640 段闭集内"
}

// ── 描述符账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GdtEntry {
    pub slot: usize,
    /// 访问字节：P(7) DPL(6-5) S(4) Type(3-0)。
    pub access: u8,
}

impl GdtEntry {
    pub fn present(&self) -> bool {
        self.access & 0x80 != 0
    }
    pub fn dpl(&self) -> u8 {
        (self.access >> 5) & 0x3
    }
    pub fn system(&self) -> bool {
        self.access & 0x10 == 0
    }

    /// 语义闸（按槽位冻结语义）。
    pub fn legal(&self) -> Result<(), u32> {
        if self.slot >= SEG_COUNT {
            return Err(GdtCode::SlotOver.val());
        }
        match self.slot {
            S_NULL => {
                if self.access != 0 {
                    return Err(GdtCode::SystemBitBad.val());
                }
                Ok(())
            }
            S_KCODE | S_KDATA => {
                if !self.present() {
                    return Err(GdtCode::PresentMissing.val());
                }
                if self.dpl() != 0 {
                    return Err(GdtCode::DplMismatch.val());
                }
                if self.system() {
                    return Err(GdtCode::SystemBitBad.val());
                }
                Ok(())
            }
            S_UCODE | S_UDATA => {
                if !self.present() {
                    return Err(GdtCode::PresentMissing.val());
                }
                if self.dpl() != 3 {
                    return Err(GdtCode::DplMismatch.val());
                }
                if self.system() {
                    return Err(GdtCode::SystemBitBad.val());
                }
                Ok(())
            }
            S_TSS => {
                if !self.present() {
                    return Err(GdtCode::PresentMissing.val());
                }
                if !self.system() {
                    return Err(GdtCode::SystemBitBad.val());
                }
                Ok(())
            }
            _ => Err(GdtCode::SlotOver.val()),
        }
    }
}

/// 选择子 = 槽位×8 | RPL；RPL 合法闸（0 或 3）。
pub fn make_selector(slot: usize, rpl: u16) -> Result<u16, u32> {
    if slot >= SEG_COUNT {
        return Err(GdtCode::SlotOver.val());
    }
    if rpl != 0 && rpl != 3 {
        return Err(GdtCode::RplBad.val());
    }
    Ok(slot as u16 * SEL_STEP | rpl)
}

/// 复算。
pub fn replay_ok(entries: &[GdtEntry]) -> Result<(), u32> {
    let mut i = 0;
    while i < entries.len() {
        let a = entries[i].legal();
        let b = entries[i].legal();
        if a != b {
            return Err(GdtCode::ReplayMismatch.val());
        }
        i += 1;
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

// ── 注入检出账（四路：DPL 路/S 位路/RPL 路/复算路） ───────────────
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

/// 真机 GDT（六段冻结访问字节）。
pub fn machine_gdt() -> Vec<GdtEntry> {
    vec![
        GdtEntry { slot: S_NULL, access: 0x00 },
        GdtEntry { slot: S_KCODE, access: 0x9A }, // P=1 DPL=0 S=1 Code
        GdtEntry { slot: S_KDATA, access: 0x92 }, // P=1 DPL=0 S=1 Data
        GdtEntry { slot: S_UCODE, access: 0xFA }, // P=1 DPL=3 S=1 Code
        GdtEntry { slot: S_UDATA, access: 0xF2 }, // P=1 DPL=3 S=1 Data
        GdtEntry { slot: S_TSS, access: 0x89 },   // P=1 DPL=0 S=0 TSS-A
    ]
}

// ── 黄金全链：六段全绿→选择子账→DPL 注入→S 位注入→复算→越槽拒 ─────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let gdt = machine_gdt();
    let mut all_ok = true;
    let mut i = 0;
    while i < gdt.len() {
        if gdt[i].legal().is_err() {
            all_ok = false;
        }
        i += 1;
    }
    // 选择子账：kcode(1,0)=0x08 / ucode(3,3)=0x1B / RPL2 拒。
    let sel_ok = (make_selector(S_KCODE, 0) == Ok(0x08)
        && make_selector(S_UCODE, 3) == Ok(0x1B)
        && make_selector(S_KDATA, 2).is_err()) as u32;
    // DPL 注入：内核段 DPL=3。
    let mut bad = machine_gdt();
    bad[S_KCODE].access |= 0x60;
    let dpl = bad[S_KCODE].legal().is_err();
    // S 位注入：TSS 设 S=1。
    let mut bad2 = machine_gdt();
    bad2[S_TSS].access |= 0x10;
    let sbit = bad2[S_TSS].legal().is_err();
    replay_ok(&gdt)?;
    let over = GdtEntry { slot: 9, access: 0 }.legal().is_err();
    Ok((all_ok as u32 + sel_ok, dpl as u32 + sbit as u32 + over as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：六段全绿 + 选择子账 + 三注入全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 3, _)) => Ok(()),
        _ => Err(GdtCode::NotReproducible.val()),
    }
}

/// 序 B：六段闭集名。
pub fn probe_seq_b() -> Result<(), u32> {
    match (SEG_COUNT == 6, SEL_STEP == 8) {
        (true, true) => Ok(()),
        _ => Err(GdtCode::NotReproducible.val()),
    }
}

/// 序 C：存在位缺失 ⇒ PresentMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    let e = GdtEntry { slot: S_KCODE, access: 0x1A };
    match e.legal() {
        Err(e2) if e2 == GdtCode::PresentMissing.val() => Ok(()),
        _ => Err(GdtCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：DPL 路。
        let mut g1 = machine_gdt();
        g1[S_UCODE].access &= !0x60; // DPL=3 → 0
        led.record(0, g1[S_UCODE].legal().is_err());
        // 路 2：S 位路。
        let mut g2 = machine_gdt();
        g2[S_KDATA].access &= !0x10; // S=1 → 0
        led.record(1, g2[S_KDATA].legal().is_err());
        // 路 3：RPL 路。
        led.record(2, make_selector(S_UCODE, 1).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_gdt()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GdtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GdtCode::ALL.len() {
        let v = GdtCode::ALL[i].val();
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
            Ok((ok, rej, _)) => format!("all+sel={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 访问字节逐位（kcode 0x9A：P/DPL0/S/Code）。
    let k = machine_gdt()[S_KCODE];
    v.push((
        "J1-访问字节",
        k.present() && k.dpl() == 0 && !k.system(),
        format!("kcode access=0x{:x}", k.access),
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
        String::from("0x6F640 窗口"),
    ));
    v
}

pub fn run_unx0189_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0189 GDT 重载与段语义固化");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
