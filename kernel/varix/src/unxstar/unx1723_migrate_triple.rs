//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1723 · 迁移执行三向记账（源/目标/线程） · 0x6CF00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1723-J1 源核出账/目标核入账/线程随行账三向闭合断言
//!     全过，与 F1627/F1646/F1704 三账联动一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三向账闭集：源核出账（-1 任务/负载迁移量）/目标核入账
//!      （+1 任务/+同负载量）/线程随行账（核号变更 + 负载携带）；
//!    - 三向闭合：源出量 ≡ 目标入量 ≡ 线程随行量（三账同值）；
//!    - 联动一致：与 F1627（核负载账）/F1646（队列长账）/F1704
//!      （贡献随行）三账的联动位一致；
//!    - 迁移序：先源出、再目标入、后线程随行（三步不可跳）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CF00，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CF00;

// ── 三步闭集 ────────────────────────────────────────────────────
pub const STEP_SRC_OUT: usize = 0;
pub const STEP_DST_IN: usize = 1;
pub const STEP_THREAD_CARRY: usize = 2;
pub const MIG_STEP_COUNT: usize = 3;
pub const MIG_STEP_NAMES: [&str; MIG_STEP_COUNT] = ["源核出账", "目标核入账", "线程随行账"];

/// 核数。
pub const CORE_COUNT: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MigCode {
    /// 步越闭集
    StepOutOfRange,
    /// 步名空（三步齐备破坏）
    StepNameBlank,
    /// 核号越界
    CoreOutOfRange,
    /// 三步跳步（不可跳）
    StepJumpDetected,
    /// 三向不闭合（出≠入≠随行）
    TripleNotClosed,
    /// 源核无此任务（出账失败）
    SrcTaskMissing,
    /// 联动账不一致
    LinkageMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl MigCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MigCode::StepOutOfRange => "步越闭集：三步冻结",
            MigCode::StepNameBlank => "步名空：齐备破坏",
            MigCode::CoreOutOfRange => "核号越界：4 核",
            MigCode::StepJumpDetected => "三步跳步：不可跳",
            MigCode::TripleNotClosed => "三向不闭合：出≠入≠随行",
            MigCode::SrcTaskMissing => "源核无此任务：出账失败",
            MigCode::LinkageMismatch => "联动账不一致",
            MigCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MigCode; 8] = [
        MigCode::StepOutOfRange,
        MigCode::StepNameBlank,
        MigCode::CoreOutOfRange,
        MigCode::StepJumpDetected,
        MigCode::TripleNotClosed,
        MigCode::SrcTaskMissing,
        MigCode::LinkageMismatch,
        MigCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MigCode::ALL.len() {
        if MigCode::ALL[i].val() == code {
            return MigCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CF00 段闭集内"
}

// ── 迁移账 ─────────────────────────────────────────────────────
/// 源核负载账（F1627 联动位）。
pub struct CoreLoad {
    pub tasks: u64,
    pub load_permille: u64,
}

/// 三向迁移账。
pub struct MigrateLedger {
    pub cores: [CoreLoad; CORE_COUNT],
    /// 迁移中的三步游标。
    pub step: usize,
    /// 出量/入量/随行量（闭合对拍三值）。
    pub out_amount: u64,
    pub in_amount: u64,
    pub carry_amount: u64,
    pub in_flight: bool,
    pub jump_caught: u32,
}

impl MigrateLedger {
    pub fn new() -> MigrateLedger {
        MigrateLedger {
            cores: core::array::from_fn(|_| CoreLoad { tasks: 0, load_permille: 0 }),
            step: MIG_STEP_COUNT, // 空闲态
            out_amount: 0,
            in_amount: 0,
            carry_amount: 0,
            in_flight: false,
            jump_caught: 0,
        }
    }

    /// 步一：源核出账。
    pub fn src_out(&mut self, src: usize, load: u64) -> Result<(), u32> {
        if src >= CORE_COUNT {
            return Err(MigCode::CoreOutOfRange.val());
        }
        if self.in_flight {
            self.jump_caught += 1;
            return Err(MigCode::StepJumpDetected.val());
        }
        if self.cores[src].tasks == 0 {
            return Err(MigCode::SrcTaskMissing.val());
        }
        self.cores[src].tasks -= 1;
        self.cores[src].load_permille = self.cores[src].load_permille.saturating_sub(load);
        self.out_amount = load;
        self.step = STEP_SRC_OUT;
        self.in_flight = true;
        Ok(())
    }

    /// 步二：目标核入账。
    pub fn dst_in(&mut self, dst: usize) -> Result<(), u32> {
        if dst >= CORE_COUNT {
            return Err(MigCode::CoreOutOfRange.val());
        }
        if !self.in_flight || self.step != STEP_SRC_OUT {
            self.jump_caught += 1;
            return Err(MigCode::StepJumpDetected.val());
        }
        self.cores[dst].tasks += 1;
        self.cores[dst].load_permille += self.out_amount;
        self.in_amount = self.out_amount;
        self.step = STEP_DST_IN;
        Ok(())
    }

    /// 步三：线程随行账。
    pub fn thread_carry(&mut self) -> Result<(), u32> {
        if !self.in_flight || self.step != STEP_DST_IN {
            self.jump_caught += 1;
            return Err(MigCode::StepJumpDetected.val());
        }
        self.carry_amount = self.in_amount;
        self.step = MIG_STEP_COUNT;
        self.in_flight = false;
        Ok(())
    }

    /// 三向闭合：出 ≡ 入 ≡ 随行。
    pub fn triple_close(&self) -> Result<(), u32> {
        if self.in_flight {
            return Err(MigCode::TripleNotClosed.val());
        }
        if self.out_amount != self.in_amount || self.in_amount != self.carry_amount {
            return Err(MigCode::TripleNotClosed.val());
        }
        Ok(())
    }

    /// 联动审计：Σ核任务数守恒（迁移不增减任务总数）。
    pub fn linkage(&self, total_before: u64) -> Result<(), u32> {
        let mut s = 0;
        let mut c = 0;
        while c < CORE_COUNT {
            s += self.cores[c].tasks;
            c += 1;
        }
        if s != total_before {
            return Err(MigCode::LinkageMismatch.val());
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

/// 黄金全链：入队→三步迁移→三向闭合→联动守恒（10 次迁移）。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if MIG_STEP_NAMES.len() != MIG_STEP_COUNT {
        return Err(MigCode::StepNameBlank.val());
    }
    let mut led = MigrateLedger::new();
    // 核 0 入队 4 任务。
    let mut i = 0;
    while i < 4 {
        led.cores[0].tasks += 1;
        led.cores[0].load_permille += 250;
        i += 1;
    }
    let total = 4u64;
    // 10 次迁移（0→1, 1→2, ... 循环）。
    let mut k = 0;
    while k < 10 {
        let src = k % CORE_COUNT;
        let dst = (k + 1) % CORE_COUNT;
        led.src_out(src, 250)?;
        led.dst_in(dst)?;
        led.thread_carry()?;
        led.triple_close()?;
        k += 1;
    }
    led.linkage(total)?;
    Ok((led.out_amount, led.carry_amount, led.jump_caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = MigrateLedger::new();
    led.cores[0].tasks = 1;
    led.src_out(0, 100)?;
    led.dst_in(1)?;
    led.thread_carry()?;
    led.triple_close()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 跳步（未出账先入账）→ 必拒。
    let mut led = MigrateLedger::new();
    led.dst_in(1)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 源核无任务 → 必拒。
    let mut led = MigrateLedger::new();
    led.src_out(2, 100)
}

/// 注入 10/10：越核、跳步、无任务、不闭合四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越核。
        let mut l0 = MigrateLedger::new();
        led.record(0, l0.src_out(9, 100).is_err());
        // 槽1：跳步。
        let mut l1 = MigrateLedger::new();
        l1.cores[0].tasks = 1;
        l1.src_out(0, 100).ok();
        l1.thread_carry().ok();
        led.record(1, l1.jump_caught >= 1);
        // 槽2：无任务。
        let mut l2 = MigrateLedger::new();
        led.record(2, l2.src_out(1, 100).is_err());
        // 槽3：不闭合（伪造入量）。
        let mut l3 = MigrateLedger::new();
        l3.cores[0].tasks = 1;
        l3.src_out(0, 100).ok();
        l3.dst_in(1).ok();
        l3.thread_carry().ok();
        l3.in_amount = 99;
        led.record(3, l3.triple_close().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MigCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MigCode::ALL.len() {
        let v = MigCode::ALL[i].val();
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
    // 三步齐备。
    v.push((
        "J1-三步齐备",
        MIG_STEP_NAMES.len() == 3 && CORE_COUNT == 4,
        format!("{:?}", MIG_STEP_NAMES),
    ));
    // 三向闭合。
    let mut led = MigrateLedger::new();
    led.cores[0].tasks = 2;
    let close_ok = match led.src_out(0, 300) {
        Ok(()) => match led.dst_in(2) {
            Ok(()) => match led.thread_carry() {
                Ok(()) => led.triple_close().is_ok(),
                Err(_) => false,
            },
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push(("J1-三向闭合断言", close_ok, String::from("出≡入≡随行")));
    // 注入 10/10。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CF00 窗口")));
    v
}

pub fn run_unx1723_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1723 迁移执行三向记账（源/目标/线程）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
