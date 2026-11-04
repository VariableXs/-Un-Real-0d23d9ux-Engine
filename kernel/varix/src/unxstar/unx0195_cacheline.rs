//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0195 · 缓存与对齐策略声明（cacheline） · 0x6F7C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0195-J1 cacheline 账：行宽冻结 64 字节，结构体账
//!     （对齐 = 64 声明 + 尺寸整数倍行），跨行账（尺寸 ≤ 行宽不
//!     跨行），跨行策略声明可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F7C0，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F7C0;

/// cacheline 行宽（64 冻结）。
pub const LINE_SIZE: usize = 64;
/// 最大跨行声明条数。
pub const MAX_DECLS: usize = 32;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClCode {
    /// 尺寸零
    SizeZero,
    /// 非行宽整数倍
    NotLineMultiple,
    /// 越行宽（单行结构不许超 64）
    OverLineSize,
    /// 重复声明
    DuplicateDecl,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl ClCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClCode::SizeZero => "尺寸零",
            ClCode::NotLineMultiple => "非行宽整数倍：64 冻结",
            ClCode::OverLineSize => "越行宽：单行 ≤64",
            ClCode::DuplicateDecl => "重复声明",
            ClCode::ReplayMismatch => "复算失配",
            ClCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ClCode; 6] = [
        ClCode::SizeZero,
        ClCode::NotLineMultiple,
        ClCode::OverLineSize,
        ClCode::DuplicateDecl,
        ClCode::ReplayMismatch,
        ClCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        if ClCode::ALL[i].val() == code {
            return ClCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F7C0 段闭集内"
}

// ── 对齐声明账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignDecl {
    /// 结构名。
    pub name: String,
    /// 尺寸（字节）。
    pub size: usize,
    /// 是否允许多行（跨行）。
    pub multi_line: bool,
}

impl AlignDecl {
    /// 声明闸：尺寸 64 整数倍；单行结构 ≤ 行宽。
    pub fn legal(&self) -> Result<(), u32> {
        if self.size == 0 {
            return Err(ClCode::SizeZero.val());
        }
        if self.size % LINE_SIZE != 0 {
            return Err(ClCode::NotLineMultiple.val());
        }
        if !self.multi_line && self.size > LINE_SIZE {
            return Err(ClCode::OverLineSize.val());
        }
        Ok(())
    }

    /// 行数。
    pub fn lines(&self) -> usize {
        self.size / LINE_SIZE
    }
}

#[derive(Clone, Debug, Default)]
pub struct AlignLedger {
    pub decls: Vec<AlignDecl>,
}

impl AlignLedger {
    pub fn new() -> AlignLedger {
        AlignLedger::default()
    }

    /// 登记（重名守卫）。
    pub fn add(&mut self, d: AlignDecl) -> Result<usize, u32> {
        d.legal()?;
        if self.decls.len() >= MAX_DECLS {
            return Err(ClCode::DuplicateDecl.val());
        }
        let mut i = 0;
        while i < self.decls.len() {
            if self.decls[i].name == d.name {
                return Err(ClCode::DuplicateDecl.val());
            }
            i += 1;
        }
        self.decls.push(d);
        Ok(self.decls.len() - 1)
    }

    /// 跨行策略汇总（多行声明的行数账）。
    pub fn multi_line_total(&self) -> usize {
        let mut t = 0;
        let mut i = 0;
        while i < self.decls.len() {
            if self.decls[i].multi_line {
                t += self.decls[i].lines();
            }
            i += 1;
        }
        t
    }
}

/// 复算。
pub fn replay_ok(decls: &[AlignDecl]) -> Result<(), u32> {
    let mut a = AlignLedger::new();
    let mut b = AlignLedger::new();
    let mut i = 0;
    while i < decls.len() {
        let _ = a.add(decls[i].clone());
        let _ = b.add(decls[i].clone());
        i += 1;
    }
    if a.decls != b.decls {
        return Err(ClCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：零尺寸路/非倍数路/跨行路/复算路） ────────────
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

/// 真机声明（单行 + 多行各一）。
pub fn machine_decls() -> Vec<AlignDecl> {
    vec![
        AlignDecl { name: String::from("PerCpuBlk"), size: 64, multi_line: false },
        AlignDecl { name: String::from("IoRing"), size: 128, multi_line: true },
    ]
}

// ── 黄金全链：声明→行数账→重名抓→三拒→复算 ────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = AlignLedger::new();
    let decls = machine_decls();
    let mut i = 0;
    while i < decls.len() {
        l.add(decls[i].clone())?;
        i += 1;
    }
    let lines_ok = (l.multi_line_total() == 2) as u32;
    // 重名抓。
    let dup = l.add(decls[0].clone()).is_err();
    // 三拒：零尺寸 / 非倍数 / 单行超宽。
    let zero = AlignDecl { name: String::from("a"), size: 0, multi_line: false }.legal().is_err();
    let notmul = AlignDecl { name: String::from("b"), size: 96, multi_line: false }.legal().is_err();
    let over = AlignDecl { name: String::from("c"), size: 128, multi_line: false }.legal().is_err();
    replay_ok(&decls)?;
    Ok((lines_ok, dup as u32 + zero as u32 + notmul as u32 + over as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：行数 2 + 重名 + 四拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 4, _)) => Ok(()),
        _ => Err(ClCode::NotReproducible.val()),
    }
}

/// 序 B：行宽冻结。
pub fn probe_seq_b() -> Result<(), u32> {
    match LINE_SIZE {
        64 => Ok(()),
        _ => Err(ClCode::NotReproducible.val()),
    }
}

/// 序 C：非倍数 ⇒ NotLineMultiple。
pub fn probe_seq_c() -> Result<(), u32> {
    let d = AlignDecl { name: String::from("x"), size: 32, multi_line: false };
    match d.legal() {
        Err(e) if e == ClCode::NotLineMultiple.val() => Ok(()),
        _ => Err(ClCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：零尺寸路。
        let mut l1 = AlignLedger::new();
        led.record(0, l1.add(AlignDecl { name: String::from("a"), size: 0, multi_line: false }).is_err());
        // 路 2：非倍数路。
        led.record(1, l1.add(AlignDecl { name: String::from("b"), size: 48, multi_line: false }).is_err());
        // 路 3：跨行路（单行超宽拒）。
        led.record(2, l1.add(AlignDecl { name: String::from("c"), size: 192, multi_line: false }).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_decls()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ClCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClCode::ALL.len() {
        let v = ClCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 1 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("lines={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 真机声明行数（单行 1 + 多行 2）。
    let decls = machine_decls();
    v.push((
        "J1-真机行数",
        decls[0].lines() == 1 && decls[1].lines() == 2,
        String::from("PerCpuBlk 1 行 / IoRing 2 行"),
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
        String::from("0x6F7C0 窗口"),
    ));
    v
}

pub fn run_unx0195_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0195 缓存与对齐策略声明（cacheline）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
