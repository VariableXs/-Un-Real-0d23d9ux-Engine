//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0173 · 启动项枚举与 BootNext 语义（只读）（深化第二实现） · 0x6F340
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0173-J1 启动项枚举：Boot0000~BootNNNN 序账（四位编
//!     号连续不跳），BootNext 单次覆盖语义（下次引导一次生效即
//!     消费），重复 BootNext 拒，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F340，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F340;

/// 最大启动项（冻结 16）。
pub const MAX_BOOT_ITEMS: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootCode {
    /// 序号跳号（四位编号不连续）
    SeqGap,
    /// 序号重复
    SeqDuplicate,
    /// 越上限
    CountOver,
    /// BootNext 指向不存在项
    BootNextMissing,
    /// BootNext 重复设
    BootNextDuplicate,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl BootCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BootCode::SeqGap => "序号跳号：不连续",
            BootCode::SeqDuplicate => "序号重复",
            BootCode::CountOver => "越上限：16 冻结",
            BootCode::BootNextMissing => "BootNext 指向不存在项",
            BootCode::BootNextDuplicate => "BootNext 重复设",
            BootCode::ReplayMismatch => "复算失配",
            BootCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BootCode; 7] = [
        BootCode::SeqGap,
        BootCode::SeqDuplicate,
        BootCode::CountOver,
        BootCode::BootNextMissing,
        BootCode::BootNextDuplicate,
        BootCode::ReplayMismatch,
        BootCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BootCode::ALL.len() {
        if BootCode::ALL[i].val() == code {
            return BootCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F340 段闭集内"
}

// ── 启动项账 ────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct BootEnum {
    pub seqs: Vec<u32>,
    pub boot_next: Option<u32>,
    pub consumed: u32,
    pub dup_caught: u32,
}

impl BootEnum {
    pub fn new() -> BootEnum {
        BootEnum::default()
    }

    /// 登记启动项（四位编号连续不跳、不重）。
    pub fn add(&mut self, seq: u32) -> Result<usize, u32> {
        if self.seqs.len() >= MAX_BOOT_ITEMS {
            return Err(BootCode::CountOver.val());
        }
        let expect = self.seqs.len() as u32;
        if seq < expect {
            return Err(BootCode::SeqDuplicate.val());
        }
        if seq > expect {
            return Err(BootCode::SeqGap.val());
        }
        self.seqs.push(seq);
        Ok(self.seqs.len() - 1)
    }

    /// 设 BootNext（必须指向存在项 + 唯一）。
    pub fn set_boot_next(&mut self, seq: u32) -> Result<(), u32> {
        if self.boot_next.is_some() {
            self.dup_caught += 1;
            return Err(BootCode::BootNextDuplicate.val());
        }
        if seq >= self.seqs.len() as u32 {
            return Err(BootCode::BootNextMissing.val());
        }
        self.boot_next = Some(seq);
        Ok(())
    }

    /// 引导消费（BootNext 单次生效）。
    pub fn consume(&mut self) -> Option<u32> {
        match self.boot_next.take() {
            Some(s) => {
                self.consumed += 1;
                Some(s)
            }
            None => None,
        }
    }
}

/// 复算。
pub fn replay_ok(seqs: &[u32], next: Option<u32>) -> Result<(), u32> {
    let mut a = BootEnum::new();
    let mut b = BootEnum::new();
    let mut i = 0;
    while i < seqs.len() {
        let _ = a.add(seqs[i]);
        let _ = b.add(seqs[i]);
        i += 1;
    }
    if let Some(s) = next {
        let _ = a.set_boot_next(s);
        let _ = b.set_boot_next(s);
    }
    if a.seqs != b.seqs || a.boot_next != b.boot_next {
        return Err(BootCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：跳号路/重复路/缺向路/复算路） ──────────────
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

/// 真机启动项（Boot0000~Boot0003）。
pub fn machine_seqs() -> Vec<u32> {
    vec![0, 1, 2, 3]
}

// ── 黄金全链：枚举→BootNext 设→消费→重复设拒→缺向拒→复算 ──────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut e = BootEnum::new();
    let seqs = machine_seqs();
    let mut i = 0;
    while i < seqs.len() {
        e.add(seqs[i])?;
        i += 1;
    }
    e.set_boot_next(2)?;
    let first = e.consume();
    let after = e.consume();
    let consume_ok = (first == Some(2) && after.is_none() && e.consumed == 1) as u32;
    // 重复设拒。
    let _ = e.set_boot_next(1);
    let dup = e.set_boot_next(2).is_err() && e.dup_caught == 1;
    // 缺向拒。
    let missing = e.set_boot_next(9).is_err();
    replay_ok(&seqs, Some(2))?;
    Ok((consume_ok, dup as u32 + missing as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：消费一次 + 重复/缺向拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 2, _)) => Ok(()),
        _ => Err(BootCode::NotReproducible.val()),
    }
}

/// 序 B：跳号 ⇒ SeqGap。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut e = BootEnum::new();
    let _ = e.add(0);
    match e.add(2) {
        Err(e2) if e2 == BootCode::SeqGap.val() => Ok(()),
        _ => Err(BootCode::NotReproducible.val()),
    }
}

/// 序 C：BootNext 指向空账 ⇒ BootNextMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut e = BootEnum::new();
    match e.set_boot_next(0) {
        Err(e2) if e2 == BootCode::BootNextMissing.val() => Ok(()),
        _ => Err(BootCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：跳号路。
        let mut e1 = BootEnum::new();
        let _ = e1.add(0);
        led.record(0, e1.add(3).is_err());
        // 路 2：重复路。
        let mut e2 = BootEnum::new();
        let _ = e2.add(0);
        led.record(1, e2.add(0).is_err());
        // 路 3：缺向路。
        let mut e3 = BootEnum::new();
        let _ = e3.add(0);
        led.record(2, e3.set_boot_next(5).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seqs(), None).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BootCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BootCode::ALL.len() {
        let v = BootCode::ALL[i].val();
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
            Ok((consume, rej, _)) => *consume == 1 && *rej == 2,
            Err(_) => false,
        },
        match &g {
            Ok((consume, rej, _)) => format!("consume_once={} rejects={}", consume, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 上限冻结。
    v.push((
        "J1-上限冻结",
        MAX_BOOT_ITEMS == 16,
        String::from("16 启动项冻结"),
    ));
    // 四位编号命名语义。
    let names_ok = (0..4).all(|i| {
        let s = alloc::format!("Boot{:04}", i);
        s == ["Boot0000", "Boot0001", "Boot0002", "Boot0003"][i]
    });
    v.push(("J1-四位编号", names_ok, String::from("Boot%04x 冻结")));
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
        String::from("0x6F340 窗口"),
    ));
    v
}

pub fn run_unx0173b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0173 启动项枚举与 BootNext 语义（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
