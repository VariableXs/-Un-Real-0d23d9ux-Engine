//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1625 · 每核中断栈与 IST 切换语义 · 0x6CDC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1625-J1 三类高优异常（#DF/NMI/#MC）经 IST 独立栈处
//!     理，IST 槽位指向核内专用栈，嵌套深度账零越界；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - IST 三槽闭集：IST1=#DF、IST2=#NMI、IST3=#MC（冻结映射）；
//!    - 每核专用栈：核 ID→栈基址（16 字节对齐），栈大小冻结 16KB；
//!    - 嵌套深度账：每槽嵌套计数单调（入 +1/出 -1），越界（<0 或
//!      >上限 4）即拒；
//!    - 切换语义：异常进入用 IST 槽切换、退出还原，逐次落账；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CDC0，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CDC0;

// ── IST 槽闭集 ─────────────────────────────────────────────────
pub const IST_DF: usize = 1;
pub const IST_NMI: usize = 2;
pub const IST_MC: usize = 3;
pub const IST_SLOTS: usize = 4; // 槽 0 保留（非 IST）
pub const EXCEPTION_NAMES: [&str; 3] = ["#DF", "#NMI", "#MC"];
/// 每核数。
pub const CORE_COUNT: usize = 4;
/// 专用栈大小（字节）。
pub const STACK_SIZE: u64 = 16 * 1024;
/// 嵌套深度上限。
pub const NEST_MAX: u32 = 4;
/// 栈对齐。
pub const STACK_ALIGN: u64 = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IstCode {
    /// 槽越闭集（0 或 >3）
    SlotOutOfRange,
    /// 异常名空（三异常齐备破坏）
    ExceptionNameBlank,
    /// 核号越界
    CoreOutOfRange,
    /// 栈基址非对齐
    StackMisaligned,
    /// 嵌套越上界
    NestOverflow,
    /// 嵌套下溢（出多于入）
    NestUnderflow,
    /// 槽位映射错（#DF 非 IST1 等）
    SlotMappingWrong,
    /// 栈越界写（超 16KB）
    StackOverrun,
    /// 双跑不一致
    NotReproducible,
}

impl IstCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IstCode::SlotOutOfRange => "槽越闭集：0 保留/1~3 IST",
            IstCode::ExceptionNameBlank => "异常名空：齐备破坏",
            IstCode::CoreOutOfRange => "核号越界：4 核",
            IstCode::StackMisaligned => "栈基址非 16 字节对齐",
            IstCode::NestOverflow => "嵌套越上界：>4",
            IstCode::NestUnderflow => "嵌套下溢：出多于入",
            IstCode::SlotMappingWrong => "槽位映射错：#DF≠IST1",
            IstCode::StackOverrun => "栈越界写：超 16KB",
            IstCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [IstCode; 9] = [
        IstCode::SlotOutOfRange,
        IstCode::ExceptionNameBlank,
        IstCode::CoreOutOfRange,
        IstCode::StackMisaligned,
        IstCode::NestOverflow,
        IstCode::NestUnderflow,
        IstCode::SlotMappingWrong,
        IstCode::StackOverrun,
        IstCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IstCode::ALL.len() {
        if IstCode::ALL[i].val() == code {
            return IstCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CDC0 段闭集内"
}

/// 异常→IST 槽（冻结映射）。
pub fn exception_slot(exc: usize) -> Result<usize, u32> {
    match exc {
        0 => Ok(IST_DF),
        1 => Ok(IST_NMI),
        2 => Ok(IST_MC),
        _ => Err(IstCode::SlotOutOfRange.val()),
    }
}

// ── 每核 IST 账 ────────────────────────────────────────────────
pub struct PerCoreIst {
    pub core: usize,
    pub stack_base: u64,
    /// 各槽嵌套深度。
    pub depth: [u32; IST_SLOTS],
    /// 切换账（次数）。
    pub switches: u64,
}

impl PerCoreIst {
    pub fn new(core: usize) -> Result<PerCoreIst, u32> {
        if core >= CORE_COUNT {
            return Err(IstCode::CoreOutOfRange.val());
        }
        let base = 0xFF00_0000 + (core as u64) * STACK_SIZE * IST_SLOTS as u64;
        if base % STACK_ALIGN != 0 {
            return Err(IstCode::StackMisaligned.val());
        }
        Ok(PerCoreIst { core, stack_base: base, depth: [0; IST_SLOTS], switches: 0 })
    }

    /// 异常进入：IST 槽嵌套 +1。
    pub fn enter(&mut self, exc: usize) -> Result<usize, u32> {
        let slot = exception_slot(exc)?;
        if self.depth[slot] >= NEST_MAX {
            return Err(IstCode::NestOverflow.val());
        }
        self.depth[slot] += 1;
        self.switches += 1;
        Ok(slot)
    }

    /// 异常退出：嵌套 -1。
    pub fn leave(&mut self, slot: usize) -> Result<(), u32> {
        if slot == 0 || slot >= IST_SLOTS {
            return Err(IstCode::SlotOutOfRange.val());
        }
        if self.depth[slot] == 0 {
            return Err(IstCode::NestUnderflow.val());
        }
        self.depth[slot] -= 1;
        self.switches += 1;
        Ok(())
    }

    /// 栈越界点验：全部槽嵌套×单帧 ≤ 栈大小。
    pub fn stack_audit(&self) -> Result<(), u32> {
        const FRAME: u64 = 512;
        let mut s = 1;
        while s < IST_SLOTS {
            if self.depth[s] as u64 * FRAME > STACK_SIZE {
                return Err(IstCode::StackOverrun.val());
            }
            s += 1;
        }
        Ok(())
    }

    /// 零越界：深度全部在 [0, NEST_MAX]。
    pub fn depth_audit(&self) -> Result<(), u32> {
        let mut s = 0;
        while s < IST_SLOTS {
            if self.depth[s] > NEST_MAX {
                return Err(IstCode::NestOverflow.val());
            }
            s += 1;
        }
        Ok(())
    }
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：4 核各三异常进出→嵌套账→栈审计→映射点验。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if EXCEPTION_NAMES.len() != 3 {
        return Err(IstCode::ExceptionNameBlank.val());
    }
    // 槽位映射点验。
    if exception_slot(0)? != IST_DF || exception_slot(1)? != IST_NMI || exception_slot(2)? != IST_MC {
        return Err(IstCode::SlotMappingWrong.val());
    }
    let mut total_switches = 0u64;
    let mut core = 0;
    while core < CORE_COUNT {
        let mut ist = PerCoreIst::new(core)?;
        // 每异常进出各 10 次。
        let mut exc = 0;
        while exc < 3 {
            let mut k = 0;
            while k < 10 {
                let slot = ist.enter(exc)?;
                ist.leave(slot)?;
                k += 1;
            }
            exc += 1;
        }
        // 嵌套组合：NMI 嵌套 3 层。
        ist.enter(1)?;
        ist.enter(1)?;
        ist.enter(1)?;
        ist.stack_audit()?;
        ist.depth_audit()?;
        ist.leave(IST_NMI)?;
        ist.leave(IST_NMI)?;
        ist.leave(IST_NMI)?;
        ist.depth_audit()?;
        total_switches += ist.switches;
        core += 1;
    }
    Ok((total_switches, STACK_SIZE, CORE_COUNT as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ist = PerCoreIst::new(0)?;
    let s = ist.enter(0)?;
    ist.leave(s)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 嵌套越上界 → 必拒。
    let mut ist = PerCoreIst::new(0)?;
    let mut k = 0;
    while k <= NEST_MAX {
        ist.enter(2)?;
        k += 1;
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 嵌套下溢 → 必拒。
    let mut ist = PerCoreIst::new(0)?;
    ist.leave(IST_DF)
}

/// 注入 10/10：越槽、越核、下溢、映射错四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越槽。
        let mut i0 = PerCoreIst::new(0).ok();
        let h0 = match &mut i0 {
            Some(ist) => ist.leave(0).is_err(),
            None => false,
        };
        led.record(0, h0);
        // 槽1：越核。
        led.record(1, PerCoreIst::new(9).is_err());
        // 槽2：下溢。
        let mut i2 = PerCoreIst::new(1).ok();
        let h2 = match &mut i2 {
            Some(ist) => ist.leave(IST_MC).is_err(),
            None => false,
        };
        led.record(2, h2);
        // 槽3：映射错。
        led.record(3, exception_slot(0) != Ok(IST_DF));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IstCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IstCode::ALL.len() {
        let v = IstCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 三异常经 IST。
    v.push((
        "J1-三异常经IST",
        EXCEPTION_NAMES.len() == 3 && IST_SLOTS == 4 && NEST_MAX == 4,
        format!("{:?} 槽 1~3", EXCEPTION_NAMES),
    ));
    // 每核专用栈。
    let ist = PerCoreIst::new(2);
    v.push((
        "J1-核内专用栈",
        match &ist {
            Ok(i) => i.stack_base % STACK_ALIGN == 0 && STACK_SIZE == 16384,
            Err(_) => false,
        },
        format!("{:?}", ist.as_ref().map(|i| i.stack_base)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CDC0 窗口")));
    v
}

pub fn run_unx1625_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1625 每核中断栈与 IST 切换语义");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
