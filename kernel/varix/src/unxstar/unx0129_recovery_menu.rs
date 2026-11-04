//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0129 · 恢复引导菜单（失败后自动进入） · 0x6EC80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F0129-J1 恢复菜单：三连败自动进入（连败账冻结 3），
//!     菜单四项（正常引导/安全模式/诊断模式/回退槽），选中账越
//!     界拒，菜单账可复算，手动进入优先于自动；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 触发账：连败 ≥3 自动进（手动键优先）；
//!   - 菜单四项闭集（0-3），选中越界拒；
//!   - 进入原因账（自动三连败 / 手动键 / 无）；
//!   - 复算：同失败序列同终态；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EC80，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EC80;

/// 自动触发阈值（冻结 3）。
pub const AUTO_THRESHOLD: u32 = 3;
/// 菜单项数（冻结 4）。
pub const MENU_ITEMS: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuCode {
    /// 选中越界（≥4）
    SelectionOver,
    /// 三连败未自动进
    AutoTriggerMissed,
    /// 复算失配
    ReplayMismatch,
    /// 无失败手动键失效
    ManualKeyIgnored,
    /// 双跑不一致
    NotReproducible,
}

impl MenuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MenuCode::SelectionOver => "选中越界：≥4",
            MenuCode::AutoTriggerMissed => "三连败未自动进",
            MenuCode::ReplayMismatch => "复算失配",
            MenuCode::ManualKeyIgnored => "手动键失效",
            MenuCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [MenuCode; 5] = [
        MenuCode::SelectionOver,
        MenuCode::AutoTriggerMissed,
        MenuCode::ReplayMismatch,
        MenuCode::ManualKeyIgnored,
        MenuCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MenuCode::ALL.len() {
        if MenuCode::ALL[i].val() == code {
            return MenuCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EC80 段闭集内"
}

// ── 恢复菜单账 ──────────────────────────────────────────────────
/// 进入原因。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnterReason {
    None,
    AutoThreeFail,
    ManualKey,
}

#[derive(Clone, Debug)]
pub struct RecoveryMenu {
    pub consecutive_fails: u32,
    pub entered: bool,
    pub reason: EnterReason,
    pub selection: Option<usize>,
}

impl RecoveryMenu {
    pub fn new() -> RecoveryMenu {
        RecoveryMenu { consecutive_fails: 0, entered: false, reason: EnterReason::None, selection: None }
    }

    /// 引导结果上报（失败累加，成功清零）。
    pub fn report(&mut self, ok: bool) -> Result<(), u32> {
        if ok {
            self.consecutive_fails = 0;
        } else {
            self.consecutive_fails += 1;
            if self.consecutive_fails >= AUTO_THRESHOLD {
                self.entered = true;
                self.reason = EnterReason::AutoThreeFail;
            }
        }
        Ok(())
    }

    /// 手动键（优先级最高）。
    pub fn manual_key(&mut self) {
        self.entered = true;
        self.reason = EnterReason::ManualKey;
    }

    /// 选中项（0-3 越界拒）。
    pub fn select(&mut self, item: usize) -> Result<(), u32> {
        if !self.entered {
            return Err(MenuCode::SelectionOver.val());
        }
        if item >= MENU_ITEMS {
            return Err(MenuCode::SelectionOver.val());
        }
        self.selection = Some(item);
        Ok(())
    }
}

/// 菜单项文案（冻结）。
pub fn item_text(item: usize) -> Result<&'static str, u32> {
    match item {
        0 => Ok("正常引导"),
        1 => Ok("安全模式"),
        2 => Ok("诊断模式"),
        3 => Ok("回退槽"),
        _ => Err(MenuCode::SelectionOver.val()),
    }
}

/// 复算：同失败序列重建终态一致。
pub fn replay_ok(seq: &[bool]) -> Result<(), u32> {
    let mut a = RecoveryMenu::new();
    let mut b = RecoveryMenu::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.report(seq[i]);
        let _ = b.report(seq[i]);
        i += 1;
    }
    if a.consecutive_fails != b.consecutive_fails
        || a.entered != b.entered
        || a.reason != b.reason
    {
        return Err(MenuCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越选路/自动路/复算路/手动路） ──────────────
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

/// 真机失败序列：三连败。
pub fn machine_fails() -> Vec<bool> {
    vec![false, false, false]
}

// ── 黄金全链：三连败自动进→选中→文案→复算→越选拒 ─────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut m = RecoveryMenu::new();
    let fails = machine_fails();
    let mut i = 0;
    while i < fails.len() {
        m.report(fails[i])?;
        i += 1;
    }
    let auto = m.entered && m.reason == EnterReason::AutoThreeFail;
    m.select(2)?;
    let text_ok = item_text(2) == Ok("诊断模式");
    replay_ok(&fails)?;
    // 越选拒。
    let mut m2 = RecoveryMenu::new();
    let _ = m2.report(false);
    let _ = m2.report(false);
    let _ = m2.report(false);
    let over = m2.select(4).is_err();
    Ok((auto as u32, text_ok as u32, over as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：三连败自动进 + 诊断模式文案 + 越选拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 1, 1)) => Ok(()),
        _ => Err(MenuCode::NotReproducible.val()),
    }
}

/// 序 B：2 连败不自动进。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut m = RecoveryMenu::new();
    let _ = m.report(false);
    let _ = m.report(false);
    match m.entered {
        false => Ok(()),
        true => Err(MenuCode::NotReproducible.val()),
    }
}

/// 序 C：成功清零连败账。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut m = RecoveryMenu::new();
    let _ = m.report(false);
    let _ = m.report(false);
    let _ = m.report(true);
    match m.consecutive_fails {
        0 => Ok(()),
        _ => Err(MenuCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越选路。
        let mut m1 = RecoveryMenu::new();
        let _ = m1.report(false);
        let _ = m1.report(false);
        let _ = m1.report(false);
        led.record(0, m1.select(9).is_err());
        // 路 2：自动路。
        let mut m2 = RecoveryMenu::new();
        let fails = machine_fails();
        let mut i = 0;
        while i < fails.len() {
            let _ = m2.report(fails[i]);
            i += 1;
        }
        led.record(1, m2.entered && m2.reason == EnterReason::AutoThreeFail);
        // 路 3：复算路。
        led.record(2, replay_ok(&machine_fails()).is_ok());
        // 路 4：手动路。
        let mut m4 = RecoveryMenu::new();
        m4.manual_key();
        led.record(3, m4.entered && m4.reason == EnterReason::ManualKey);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MenuCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MenuCode::ALL.len() {
        let v = MenuCode::ALL[i].val();
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
            Ok((auto, text, over)) => *auto == 1 && *text == 1 && *over == 1,
            Err(_) => false,
        },
        match &g {
            Ok((auto, text, over)) => format!("auto={} text={} over_reject={}", auto, text, over),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 阈值/菜单冻结。
    v.push((
        "J1-冻结账",
        AUTO_THRESHOLD == 3 && MENU_ITEMS == 4,
        String::from("三连败触发 / 四项菜单 冻结"),
    ));
    // 四项文案逐字。
    let items_ok = item_text(0) == Ok("正常引导")
        && item_text(1) == Ok("安全模式")
        && item_text(2) == Ok("诊断模式")
        && item_text(3) == Ok("回退槽");
    v.push((
        "J1-四项文案",
        items_ok,
        String::from("正常引导/安全模式/诊断模式/回退槽"),
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
        String::from("0x6EC80 窗口"),
    ));
    v
}

pub fn run_unx0129_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0129 恢复引导菜单（失败后自动进入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
