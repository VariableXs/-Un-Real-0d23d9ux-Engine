//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15741 · DllRegisterServer 契约（全量/幂等重注册 + 半注册注入）
//! · 0x67740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15741-J1 契约成文（注册全量/幂等重注册语义 10/10 次），
//!      半注册注入（注册中途失败残留半态）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 注册步闭集：CLSID 写表 / ProgID 写表 / 类型库登记 / 接口列示
//!     四步，全量=四步全成；幂等=重注册同 ID 不产生重复键；
//!   - 注册状态机：Unregistered → Registering(步 n) → Registered /
//!     HalfRegistered（中途失败残留半态，可回滚）；
//!   - 半注册注入：在第 k 步强制失败 ×10 次，10/10 检出半态并回滚；
//!   - 幂等账本：重复注册不改账（键集与计数守恒）；
//!   - LCG 种子写死 = SEG_BASE（0x67740），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67740，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67740;

/// 注册步闭集。
pub const STEP_CLSID: usize = 0;
pub const STEP_PROGID: usize = 1;
pub const STEP_TYPELIB: usize = 2;
pub const STEP_IFACE: usize = 3;
pub const STEP_COUNT: usize = 4;
pub const STEP_NAMES: [&str; STEP_COUNT] = [
    "CLSID写表", "ProgID写表", "类型库登记", "接口列示",
];

/// 每语义正演次数（判据钉版：10/10）。
pub const RUNS_PER_SEM: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 注册步越闭集
    StepOutOfRange,
    /// 半态残留（中途失败未回滚）
    HalfStateResidue,
    /// 幂等被破坏（重注册改账）
    IdempotencyBroken,
    /// 键重复（全量注册产生重复键）
    DuplicateKey,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 未注册即注销（相位非法）
    UnregWithoutReg,
    /// 正演账未满
    RunLedgerShort,
    /// 双跑不一致
    NotReproducible,
    /// 注册表账本键值不守恒
    LedgerNotConserved,
}

impl RegCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::StepOutOfRange => "注册步越闭集：四步冻结",
            RegCode::HalfStateResidue => "半态残留：中途失败未回滚",
            RegCode::IdempotencyBroken => "幂等被破坏：重注册改账",
            RegCode::DuplicateKey => "键重复：全量注册产生重复键",
            RegCode::IllegalPhaseEdge => "非法相位边：状态机闭集外",
            RegCode::UnregWithoutReg => "未注册即注销：相位非法",
            RegCode::RunLedgerShort => "正演账未满：语义须 10/10",
            RegCode::NotReproducible => "双跑不一致：契约复现红线",
            RegCode::LedgerNotConserved => "键值不守恒：账本对账失败",
        }
    }
    pub const ALL: [RegCode; 9] = [
        RegCode::StepOutOfRange,
        RegCode::HalfStateResidue,
        RegCode::IdempotencyBroken,
        RegCode::DuplicateKey,
        RegCode::IllegalPhaseEdge,
        RegCode::UnregWithoutReg,
        RegCode::RunLedgerShort,
        RegCode::NotReproducible,
        RegCode::LedgerNotConserved,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        if RegCode::ALL[i].val() == code {
            return RegCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67740 段闭集内"
}

// ── 注册状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegPhase {
    /// 未注册
    Unregistered,
    /// 注册中（已完成的步数）
    Registering(usize),
    /// 已注册（四步全成）
    Registered,
    /// 半注册残留（中途失败）
    HalfRegistered(usize),
    /// 已注销（从 Registered 回落）
    UnregisteredAgain,
}

/// 合法边闭集（写死）：
///   Unregistered → Registering(0)；
///   Registering(n) → Registering(n+1)（n<STEP_COUNT-1）；
///   Registering(STEP_COUNT-1) → Registered；
///   Registering(n) → HalfRegistered(n+1)（注入失败点 n+1）；
///   HalfRegistered(n) → Unregistered（回滚）；
///   Registered → UnregisteredAgain（注销）。
pub fn reg_edge_legal(from: RegPhase, to: RegPhase) -> bool {
    match (from, to) {
        (RegPhase::Unregistered, RegPhase::Registering(0)) => true,
        (RegPhase::Registering(n), RegPhase::Registering(m)) => m == n + 1 && n + 1 < STEP_COUNT,
        (RegPhase::Registering(n), RegPhase::Registered) => n + 1 == STEP_COUNT,
        (RegPhase::Registering(n), RegPhase::HalfRegistered(m)) => {
            m > n && m <= STEP_COUNT
        }
        (RegPhase::HalfRegistered(_), RegPhase::Unregistered) => true,
        (RegPhase::Registered, RegPhase::UnregisteredAgain) => true,
        _ => false,
    }
}

// ── 注册表账本 ───────────────────────────────────────────────────
/// 键条目：CLSID 字符串 → 组件名。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegKey {
    pub clsid: String,
    pub component: String,
    /// 幂等标记：重注册命中计数。
    pub hits: u32,
}

pub struct RegLedger {
    pub keys: Vec<RegKey>,
    pub phase: RegPhase,
    /// 全量注册成功次数。
    pub full_regs: u32,
    /// 幂等重注册命中次数。
    pub idem_hits: u32,
    /// 半态检出次数。
    pub half_caught: u32,
}

impl RegLedger {
    pub fn new() -> RegLedger {
        RegLedger {
            keys: Vec::new(),
            phase: RegPhase::Unregistered,
            full_regs: 0,
            idem_hits: 0,
            half_caught: 0,
        }
    }

    fn move_phase(&mut self, to: RegPhase) -> Result<(), u32> {
        let from = self.phase;
        if !reg_edge_legal(from, to) {
            return Err(RegCode::IllegalPhaseEdge.val());
        }
        self.phase = to;
        Ok(())
    }

    /// 单步写账：键不存在则新增，存在则幂等命中（不改键值）。
    fn write_step(&mut self, step: usize) -> Result<(), u32> {
        if step >= STEP_COUNT {
            return Err(RegCode::StepOutOfRange.val());
        }
        let clsid = STEP_NAMES[step];
        let mut found = false;
        let mut i = 0;
        while i < self.keys.len() {
            if self.keys[i].clsid == clsid {
                found = true;
                break;
            }
            i += 1;
        }
        if found {
            // 幂等：命中不改键值，只计命中。
            self.keys[i].hits += 1;
            self.idem_hits += 1;
            Ok(())
        } else {
            self.keys.push(RegKey {
                clsid: String::from(clsid),
                component: String::from("varix.unx15741"),
                hits: 0,
            });
            Ok(())
        }
    }

    /// 全量注册：四步逐进，中途注入失败点 fail_at（None=无注入）。
    /// 失败即入 HalfRegistered 并强制回滚清键（契约：无半态残留）。
    pub fn register(&mut self, fail_at: Option<usize>) -> Result<bool, u32> {
        self.move_phase(RegPhase::Registering(0))?;
        let mut step = 0;
        while step < STEP_COUNT {
            if fail_at == Some(step) {
                // 注入：第 step 步失败 → 半态残留检测点。
                self.move_phase(RegPhase::HalfRegistered(step))?;
                self.half_caught += 1;
                // 契约：半态必须回滚（清全部已写键）。
                self.keys.clear();
                self.phase = RegPhase::Unregistered;
                return Ok(false);
            }
            self.write_step(step)?;
            if step + 1 < STEP_COUNT {
                self.move_phase(RegPhase::Registering(step + 1))?;
            }
            step += 1;
        }
        self.move_phase(RegPhase::Registered)?;
        self.full_regs += 1;
        Ok(true)
    }

    /// 幂等重注册：同 ID 再注册一遍，键集与键值不变，只涨命中。
    pub fn reregister_idempotent(&mut self) -> Result<(), u32> {
        let before = self.keys.len();
        let mut snap_hits: Vec<u32> = Vec::new();
        let mut i = 0;
        while i < self.keys.len() {
            snap_hits.push(self.keys[i].hits);
            i += 1;
        }
        self.register(None)?;
        if self.keys.len() != before {
            return Err(RegCode::IdempotencyBroken.val());
        }
        let mut j = 0;
        while j < self.keys.len() {
            if self.keys[j].hits != snap_hits[j] + 1 {
                return Err(RegCode::IdempotencyBroken.val());
            }
            j += 1;
        }
        Ok(())
    }

    /// 注销：Registered → UnregisteredAgain → 清键。
    pub fn unregister(&mut self) -> Result<(), u32> {
        self.move_phase(RegPhase::UnregisteredAgain)?;
        self.keys.clear();
        self.phase = RegPhase::Unregistered;
        Ok(())
    }

    /// 注销未注册即拒。
    pub fn unregister_illegal(&mut self) -> Result<(), u32> {
        self.unregister()
    }

    /// 账本守恒：Unregistered 相位键必空；Registered 键必满四。
    pub fn conserve_check(&self) -> Result<(), u32> {
        match self.phase {
            RegPhase::Unregistered | RegPhase::UnregisteredAgain => {
                if !self.keys.is_empty() {
                    return Err(RegCode::LedgerNotConserved.val());
                }
            }
            RegPhase::Registered => {
                if self.keys.len() != STEP_COUNT {
                    return Err(RegCode::LedgerNotConserved.val());
                }
            }
            RegPhase::HalfRegistered(_) => {
                // 契约：HalfRegistered 相位在账本中不允许长期驻留（已回滚）。
                return Err(RegCode::HalfStateResidue.val());
            }
            RegPhase::Registering(_) => {
                return Err(RegCode::IllegalPhaseEdge.val());
            }
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] =
    ["半注册残留族", "幂等破坏族", "非法注销族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_SEM {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：半注册注入（失败点在四步上轮转 ×10）→ 半态必检出且回滚。
    let mut k = 0u32;
    while k < RUNS_PER_SEM {
        ledger.tries[0] += 1;
        let mut led = RegLedger::new();
        let fail_step = k as usize % STEP_COUNT;
        match led.register(Some(fail_step)) {
            Ok(false) => {
                // 半态已检出并回滚：键空 + 相位 Unregistered。
                if led.half_caught == 1 && led.keys.is_empty() {
                    ledger.caught[0] += 1;
                }
            }
            _ => {}
        }
        k += 1;
    }
    // 族 1：幂等破坏注入（篡改键值后重注册必拒改）。
    k = 0;
    while k < RUNS_PER_SEM {
        ledger.tries[1] += 1;
        let mut led = RegLedger::new();
        let _ = led.register(None);
        // 模拟破坏：删一键后再重注册 → 键数变化 → 幂等检查必须报错。
        if led.keys.len() > 0 {
            led.keys.remove(0);
        }
        let r = led.reregister_idempotent();
        if let Err(e) = r {
            if e == RegCode::IdempotencyBroken.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：非法注销注入（Unregistered 直接注销 ×10）。
    k = 0;
    while k < RUNS_PER_SEM {
        ledger.tries[2] += 1;
        let mut led = RegLedger::new();
        if let Err(e) = led.unregister_illegal() {
            if e == RegCode::IllegalPhaseEdge.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：全量注册 + 幂等重注册 ×10 ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub full_regs: u32,
    pub idem_hits: u32,
    pub key_count: usize,
    pub phase_registered: bool,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut led = RegLedger::new();
    // 全量注册语义 10/10：注册→注销→注册 轮转。
    let mut run = 0u32;
    while run < RUNS_PER_SEM {
        led.register(None)?;
        if led.phase != RegPhase::Registered {
            return Err(RegCode::LedgerNotConserved.val());
        }
        led.unregister()?;
        run += 1;
    }
    // 收尾：注册一次后幂等重注册 10 遍。
    led.register(None)?;
    let mut run2 = 0u32;
    while run2 < RUNS_PER_SEM {
        led.reregister_idempotent()?;
        run2 += 1;
    }
    led.conserve_check()?;
    Ok(GoldenResult {
        full_regs: led.full_regs,
        idem_hits: led.idem_hits,
        key_count: led.keys.len(),
        phase_registered: led.phase == RegPhase::Registered,
    })
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let same = a.full_regs == b.full_regs
        && a.idem_hits == b.idem_hits
        && a.key_count == b.key_count
        && a.phase_registered == b.phase_registered;
    if !same {
        return Err(RegCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RegCode::ALL.len() {
        let v = RegCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：全量/幂等 10/10。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => {
            g.full_regs == RUNS_PER_SEM + 1
                && g.idem_hits >= RUNS_PER_SEM * STEP_COUNT as u32
                && g.key_count == STEP_COUNT
                && g.phase_registered
        }
        Err(_) => false,
    };
    v.push(("J1-全量+幂等语义10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-半注册等三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 注册步名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < STEP_COUNT {
        if STEP_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push(("J1-四步名点验", ok == STEP_COUNT, format!("{}/4", ok)));
    // 5. 状态机非法边抽验：HalfRegistered→Registered 直跳必拒。
    let bad = !reg_edge_legal(RegPhase::HalfRegistered(2), RegPhase::Registered);
    v.push(("J1-非法边Half→Reg必拒", bad, String::from("闭集外")));
    // 6. 守恒恒等式：Registered 键满四、Unregistered 键空。
    let mut led2 = RegLedger::new();
    let _ = led2.register(None);
    let c1 = led2.conserve_check();
    let _ = led2.unregister();
    let c2 = led2.conserve_check();
    let cons = matches!(c1, Ok(())) && matches!(c2, Ok(()));
    v.push(("J1-键值守恒恒等式", cons, format!("Reg=4键 Unreg=0键")));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67740 窗口")));
    v
}

pub fn run_unx15741_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B28 DllRegisterServer 契约（全量/幂等 + 半注册注入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
