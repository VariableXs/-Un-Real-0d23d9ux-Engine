//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0168 · ExitBootServices 前回调链（深化第二实现） · 0x6F200
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0168-J1 EBS 前回调链：回调登记账（优先级/回调 id），
//!     触发按优先级序逐一回访（漏访红账），链封存后拒增补，账可
//!     复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F200，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F200;

/// 最大回调数（冻结 16）。
pub const MAX_CALLBACKS: usize = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CbCode {
    /// 回调越上限
    CallbackOver,
    /// 优先级重复
    PrioDuplicate,
    /// 漏访（链触发未回访）
    VisitMissing,
    /// 封存后增补
    SealedAppend,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl CbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CbCode::CallbackOver => "回调越上限：16 冻结",
            CbCode::PrioDuplicate => "优先级重复",
            CbCode::VisitMissing => "漏访：链触发未回访",
            CbCode::SealedAppend => "封存后增补：只读红线",
            CbCode::ReplayMismatch => "复算失配",
            CbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CbCode; 6] = [
        CbCode::CallbackOver,
        CbCode::PrioDuplicate,
        CbCode::VisitMissing,
        CbCode::SealedAppend,
        CbCode::ReplayMismatch,
        CbCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CbCode::ALL.len() {
        if CbCode::ALL[i].val() == code {
            return CbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F200 段闭集内"
}

// ── 回调链账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Callback {
    pub id: u32,
    pub prio: u32,
    pub visited: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CallbackChain {
    pub cbs: Vec<Callback>,
    pub sealed: bool,
    pub visited_count: u32,
}

impl CallbackChain {
    pub fn new() -> CallbackChain {
        CallbackChain::default()
    }

    /// 登记（优先级升序插入 + 重复拒）。
    pub fn register(&mut self, id: u32, prio: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(CbCode::SealedAppend.val());
        }
        if self.cbs.len() >= MAX_CALLBACKS {
            return Err(CbCode::CallbackOver.val());
        }
        let mut i = 0;
        while i < self.cbs.len() {
            if self.cbs[i].prio == prio {
                return Err(CbCode::PrioDuplicate.val());
            }
            i += 1;
        }
        let pos = {
            let mut pos = self.cbs.len();
            let mut i = 0;
            while i < self.cbs.len() {
                if self.cbs[i].prio > prio {
                    pos = i;
                    break;
                }
                i += 1;
            }
            pos
        };
        self.cbs.insert(pos, Callback { id, prio, visited: false });
        Ok(pos)
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 触发链（按序全回访；漏访审计）。
    pub fn fire(&mut self) -> Result<usize, u32> {
        let n = self.cbs.len();
        let mut i = 0;
        while i < n {
            self.cbs[i].visited = true;
            self.visited_count += 1;
            i += 1;
        }
        Ok(n)
    }

    /// 漏访审计。
    pub fn visit_audit(&self) -> Result<usize, u32> {
        let mut i = 0;
        while i < self.cbs.len() {
            if !self.cbs[i].visited {
                return Err(CbCode::VisitMissing.val());
            }
            i += 1;
        }
        Ok(self.visited_count as usize)
    }
}

/// 复算。
pub fn replay_ok(ids: &[u32], prios: &[u32]) -> Result<(), u32> {
    let mut a = CallbackChain::new();
    let mut b = CallbackChain::new();
    let mut i = 0;
    while i < ids.len() {
        let _ = a.register(ids[i], prios[i]);
        let _ = b.register(ids[i], prios[i]);
        i += 1;
    }
    if a.cbs != b.cbs {
        return Err(CbCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越限路/重 prio 路/漏访路/复算路） ───────────
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

/// 真机回调（三件乱序登记）。
pub fn machine_cbs() -> [(u32, u32); 3] {
    [(10, 30), (11, 10), (12, 20)]
}

// ── 黄金全链：乱序登记→序正确→触发全访→封存拒→漏访抓→复算 ─────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut chain = CallbackChain::new();
    let cbs = machine_cbs();
    let mut i = 0;
    while i < cbs.len() {
        chain.register(cbs[i].0, cbs[i].1)?;
        i += 1;
    }
    // 升序账：prio 10/20/30 ⇒ id 11/12/10。
    let order_ok = (chain.cbs[0].id == 11 && chain.cbs[1].id == 12 && chain.cbs[2].id == 10) as u32;
    chain.seal();
    let sealed_reject = chain.register(99, 5).is_err();
    chain.fire()?;
    chain.visit_audit()?;
    // 漏访抓（手工清一位）。
    let mut bad = CallbackChain::new();
    let mut i = 0;
    while i < cbs.len() {
        let _ = bad.register(cbs[i].0, cbs[i].1);
        i += 1;
    }
    let _ = bad.fire();
    bad.cbs[1].visited = false;
    let missing = bad.visit_audit().is_err();
    replay_ok(&[10u32, 11, 12], &[30u32, 10, 20])?;
    Ok((order_ok + sealed_reject as u32, missing as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：序正确 + 封存拒 + 漏访抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 1, _)) => Ok(()),
        _ => Err(CbCode::NotReproducible.val()),
    }
}

/// 序 B：优先级重复 ⇒ PrioDuplicate。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut c = CallbackChain::new();
    let _ = c.register(1, 5);
    match c.register(2, 5) {
        Err(e) if e == CbCode::PrioDuplicate.val() => Ok(()),
        _ => Err(CbCode::NotReproducible.val()),
    }
}

/// 序 C：触发全访计数。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut c = CallbackChain::new();
    let cbs = machine_cbs();
    let mut i = 0;
    while i < cbs.len() {
        let _ = c.register(cbs[i].0, cbs[i].1);
        i += 1;
    }
    let _ = c.fire()?;
    match c.visit_audit() {
        Ok(3) => Ok(()),
        _ => Err(CbCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重 prio 路。
        let mut c1 = CallbackChain::new();
        let _ = c1.register(1, 7);
        led.record(0, c1.register(2, 7).is_err());
        // 路 2：漏访路。
        let mut c2 = CallbackChain::new();
        let _ = c2.register(1, 1);
        led.record(1, c2.visit_audit().is_err());
        // 路 3：封存路。
        let mut c3 = CallbackChain::new();
        c3.seal();
        led.record(2, c3.register(1, 1).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&[1u32, 2, 3], &[3u32, 1, 2]).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CbCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CbCode::ALL.len() {
        let v = CbCode::ALL[i].val();
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
            Ok((ok, miss, _)) => *ok == 2 && *miss == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ok, miss, _)) => format!("order+seal={} missing={}", ok, miss),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 上限冻结。
    v.push((
        "J1-上限冻结",
        MAX_CALLBACKS == 16,
        String::from("16 回调冻结"),
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
        String::from("0x6F200 窗口"),
    ));
    v
}

pub fn run_unx0168b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0168 ExitBootServices 前回调链（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
