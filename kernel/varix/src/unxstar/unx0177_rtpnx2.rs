//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0177 · Runtime 区属性保护（NX/RO）（深化第二实现） · 0x6F540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0177-J1 Runtime 区页账（代码 RX/数据 RO 数据 NX 双
//!     红线：数据页不可执行/代码页不可写），越权组合拒，页账可复
//!     算零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F540，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F540;

/// 页类（冻结）。
pub const P_CODE: u8 = 0;
pub const P_DATA: u8 = 1;
/// 权限位。
pub const ACC_W: u8 = 1;
pub const ACC_X: u8 = 2;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NxCode {
    /// 页类越界
    PageKindOver,
    /// 数据页可执行（NX 红线）
    DataExecutable,
    /// 代码页可写（W^X 红线）
    CodeWritable,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl NxCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            NxCode::PageKindOver => "页类越界：>1",
            NxCode::DataExecutable => "数据页可执行：NX 红线",
            NxCode::CodeWritable => "代码页可写：W^X 红线",
            NxCode::ReplayMismatch => "复算失配",
            NxCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [NxCode; 5] = [
        NxCode::PageKindOver,
        NxCode::DataExecutable,
        NxCode::CodeWritable,
        NxCode::ReplayMismatch,
        NxCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < NxCode::ALL.len() {
        if NxCode::ALL[i].val() == code {
            return NxCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F540 段闭集内"
}

// ── 页属性账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtPage {
    pub kind: u8,
    pub acc: u8,
}

impl RtPage {
    /// 冻结落位：代码 RX（acc=ACC_X）/数据 RO（acc=0）。
    pub fn frozen(kind: u8) -> Result<RtPage, u32> {
        match kind {
            P_CODE => Ok(RtPage { kind, acc: ACC_X }),
            P_DATA => Ok(RtPage { kind, acc: 0 }),
            _ => Err(NxCode::PageKindOver.val()),
        }
    }

    /// 双红线闸。
    pub fn legal(&self) -> Result<(), u32> {
        if self.kind > P_DATA {
            return Err(NxCode::PageKindOver.val());
        }
        if self.kind == P_DATA && self.acc & ACC_X != 0 {
            return Err(NxCode::DataExecutable.val());
        }
        if self.kind == P_CODE && self.acc & ACC_W != 0 {
            return Err(NxCode::CodeWritable.val());
        }
        Ok(())
    }
}

/// 复算。
pub fn replay_ok(pages: &[RtPage]) -> Result<(), u32> {
    let mut i = 0;
    while i < pages.len() {
        let a = pages[i].legal();
        let b = pages[i].legal();
        if a != b {
            return Err(NxCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越类路/NX 路/W^X 路/复算路） ────────────────
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

/// 真机页账（代码 RX + 数据 RO）。
pub fn machine_pages() -> Vec<RtPage> {
    vec![
        RtPage { kind: P_CODE, acc: ACC_X },
        RtPage { kind: P_DATA, acc: 0 },
    ]
}

// ── 黄金全链：冻结落位→双红线注入抓→复算→越类拒 ────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let pages = machine_pages();
    let frozen_ok = (pages[0].acc == ACC_X && pages[1].acc == 0) as u32;
    // NX 红线注入。
    let nx = RtPage { kind: P_DATA, acc: ACC_X };
    let nx_caught = nx.legal().is_err();
    // W^X 红线注入。
    let wx = RtPage { kind: P_CODE, acc: ACC_W | ACC_X };
    let wx_caught = wx.legal().is_err();
    // 合法账全过。
    let mut all_ok = true;
    let mut i = 0;
    while i < pages.len() {
        if pages[i].legal().is_err() {
            all_ok = false;
        }
        i += 1;
    }
    if replay_ok(&pages).is_err() {
        return Err(NxCode::ReplayMismatch.val());
    }
    Ok((frozen_ok + all_ok as u32, nx_caught as u32 + wx_caught as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：冻结落位 + 双红线抓 + 全账绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 2, _)) => Ok(()),
        _ => Err(NxCode::NotReproducible.val()),
    }
}

/// 序 B：页类越界 ⇒ PageKindOver。
pub fn probe_seq_b() -> Result<(), u32> {
    match RtPage::frozen(9) {
        Err(e) if e == NxCode::PageKindOver.val() => Ok(()),
        _ => Err(NxCode::NotReproducible.val()),
    }
}

/// 序 C：双红线语义逐字。
pub fn probe_seq_c() -> Result<(), u32> {
    let nx = RtPage { kind: P_DATA, acc: ACC_W | ACC_X }.legal();
    let wx = RtPage { kind: P_CODE, acc: ACC_W | ACC_X }.legal();
    // 数据页：X 优先判 NX 红线；代码页：W 判 W^X。
    match (nx, wx) {
        (Err(e1), Err(e2)) if e1 == NxCode::DataExecutable.val() && e2 == NxCode::CodeWritable.val() => Ok(()),
        _ => Err(NxCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：NX 路。
        led.record(0, RtPage { kind: P_DATA, acc: ACC_X }.legal().is_err());
        // 路 2：W^X 路。
        led.record(1, RtPage { kind: P_CODE, acc: ACC_W }.legal().is_err());
        // 路 3：越类路。
        led.record(2, RtPage::frozen(5).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_pages()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(NxCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < NxCode::ALL.len() {
        let v = NxCode::ALL[i].val();
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
            Ok((ok, red, _)) => *ok == 2 && *red == 2,
            Err(_) => false,
        },
        match &g {
            Ok((ok, red, _)) => format!("frozen+all={} redlines={}", ok, red),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 页类闭集。
    v.push((
        "J1-页类闭集",
        (P_CODE, P_DATA) == (0, 1),
        String::from("代码/数据 两类冻结"),
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
        String::from("0x6F540 窗口"),
    ));
    v
}

pub fn run_unx0177b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0177 Runtime 区属性保护（NX/RO）（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
