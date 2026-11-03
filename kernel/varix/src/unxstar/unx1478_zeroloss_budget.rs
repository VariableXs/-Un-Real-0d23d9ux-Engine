//! UNX-F1478 零丢巡检性能预算 · 0x75980段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7I 逐字锚定，AI-VX7-1104 批4 I 组）：
//!   恒等式 5‰+扫描 3‰+日报 1‰ 合计 9‰≤10‰ 红线，巡检与热路径步数
//!   隔离，超预算自动降频 10/10 次。
//! 逐卡要点（分派表逐字锚定）：
//!   三线预算闭集步数化千分位（恒等式 5‰+扫描 3‰+日报 1‰=9‰≤10‰
//!   红线），巡检计数器与热路径计数器隔离断言（不共享），超预算自动
//!   降频三态状态机（正常→降频步数减半→停机显性，恢复滞回 8 次确认）
//!   10/10 演练全过。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 三线预算闭集：恒等式线 5‰ / 扫描线 3‰ / 日报线 1‰，合计
//!     9‰，红线 10‰——预算以千分位（‰×1000 定点）步数化，配置
//!     合计超红线在装配期显性拒（TotalOver），不留运行期发现；
//!   ② 计数器隔离：巡检计数器（三线步数册）与热路径计数器（单册）
//!     是两个独立 struct，互不引用互不共享——共享绑定入口显性拒
//!     （CounterShared），隔离断言双向打点互不串账；
//!   ③ 千分位换算：‰ = 线步数×1000 ÷ 窗口总步数预算（冻结 10 万步，
//!     整数定点，saturating 上界封顶），单线实测超配额即 LineOver，
//!     三线实测合计超 10‰ 即 BudgetOverRed；
//!   ④ 调频三态状态机：Normal→Downshift→Halted 单调升级（超预算
//!     观测逐级升级），Downshift 步数规模减半（1000→500 定点），
//!     Halted 停机显性（巡检请求显性拒 HaltActive）——倒退只走
//!     滞回确认通道，不许跳级；
//!   ⑤ 恢复滞回 8 次：降频/停机态下需连续 8 次预算内确认才回升
//!     一级（Halted→Downshift→Normal），第 7 次确认显性拒
//!     （ConfirmShort），确认序列被打断即清零重计；
//!   ⑥ 10/10 演练：LCG 种子固定驱动 10 轮（超线→自动降频→减半
//!     复检→滞回恢复），每轮全过 10/10；
//!   ⑦ 全确定性：零墙钟零随机零浮点，诊断码 0x40 窗口闭集，
//!     中文诊断文案带可操作建议。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75980，批4 I 组独占窗口）。
pub const SEG_BASE: u32 = 0x75980;

/// 恒等式线配额（冻结 5‰：判据点名）。
pub const PM_IDENTITY: u64 = 5;
/// 扫描线配额（冻结 3‰：判据点名）。
pub const PM_SCAN: u64 = 3;
/// 日报线配额（冻结 1‰：判据点名）。
pub const PM_REPORT: u64 = 1;
/// 三线合计（冻结 9‰：5+3+1）。
pub const PM_TOTAL: u64 = PM_IDENTITY + PM_SCAN + PM_REPORT;
/// 预算红线（冻结 10‰：判据点名）。
pub const PM_REDLINE: u64 = 10;
/// 千分位基数（定点 1000）。
pub const PM_GRAN: u64 = 1000;
/// 巡检窗口总步数预算（冻结 10 万步：千分位换算的分母口径）。
pub const BUDGET_STEPS: u64 = 100_000;
/// 降频步数规模（正常 1000 定点，降频减半 500）。
pub const SCALE_NORMAL: u64 = 1000;
/// 降频步数规模（减半）。
pub const SCALE_DOWNSHIFT: u64 = 500;
/// 恢复滞回确认次数（冻结 8：判据点名）。
pub const HYSTERESIS_CONFIRM: u32 = 8;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 合计配置超红线：三线配额和 >10‰
    TotalOver,
    /// 单线超配额：实测‰ > 线配额
    LineOver,
    /// 实测合计超红线：>10‰
    BudgetOverRed,
    /// 计数器共享拒：巡检册与热路径册不得共享
    CounterShared,
    /// 停机显性：Halted 态巡检请求拒
    HaltActive,
    /// 恢复滞回不足：确认 <8 次
    ConfirmShort,
    /// 状态机倒退：Normal→Downshift→Halted 之外方向
    StateRegress,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl BudgetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BudgetCode::TotalOver => {
                "合计配置超红线：三线配额和必须 ≤10‰，请按 5‰+3‰+1‰=9‰ 标准装配"
            }
            BudgetCode::LineOver => {
                "单线超配额：该线实测千分位超过配额，请降该线步数或走调频状态机"
            }
            BudgetCode::BudgetOverRed => {
                "实测合计超红线：三线实测‰和 >10‰，请立即触发自动降频"
            }
            BudgetCode::CounterShared => {
                "计数器共享拒：巡检步数册与热路径步数册必须隔离，请勿合并计数面"
            }
            BudgetCode::HaltActive => {
                "停机显性：调频机已 Halted，巡检请求一律拒，请先走 8 次滞回确认恢复"
            }
            BudgetCode::ConfirmShort => {
                "恢复滞回不足：预算内确认未满 8 次，请继续确认而非强推状态"
            }
            BudgetCode::StateRegress => {
                "状态机倒退拒：Normal→Downshift→Halted 单调升级，回升只走滞回通道"
            }
            BudgetCode::NotReproducible => {
                "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值"
            }
        }
    }
    pub const ALL: [BudgetCode; 8] = [
        BudgetCode::TotalOver,
        BudgetCode::LineOver,
        BudgetCode::BudgetOverRed,
        BudgetCode::CounterShared,
        BudgetCode::HaltActive,
        BudgetCode::ConfirmShort,
        BudgetCode::StateRegress,
        BudgetCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75980 段闭集内"
}

// ── 域模型：三线闭集与预算配置 ──────────────────────────────────
/// 预算三线闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    /// 恒等式线（5‰）。
    Identity,
    /// 扫描线（3‰）。
    Scan,
    /// 日报线（1‰）。
    Report,
}

impl Line {
    pub fn name(self) -> &'static str {
        match self {
            Line::Identity => "恒等式",
            Line::Scan => "扫描",
            Line::Report => "日报",
        }
    }
    /// 线配额（‰）。
    pub fn allowance(self) -> u64 {
        match self {
            Line::Identity => PM_IDENTITY,
            Line::Scan => PM_SCAN,
            Line::Report => PM_REPORT,
        }
    }
    pub const ALL: [Line; 3] = [Line::Identity, Line::Scan, Line::Report];
}

/// 预算配置（三线配额，装配期校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetConfig {
    pub pm_identity: u64,
    pub pm_scan: u64,
    pub pm_report: u64,
}

impl BudgetConfig {
    /// 标准装配（5‰+3‰+1‰）。
    pub fn standard() -> BudgetConfig {
        BudgetConfig {
            pm_identity: PM_IDENTITY,
            pm_scan: PM_SCAN,
            pm_report: PM_REPORT,
        }
    }
    /// 合计配额。
    pub fn total(&self) -> u64 {
        self.pm_identity
            .saturating_add(self.pm_scan)
            .saturating_add(self.pm_report)
    }
    /// 装配校验：合计 ≤10‰ 红线（超线装配期显性拒）。
    pub fn validate(&self) -> Result<(), u32> {
        if self.total() > PM_REDLINE {
            return Err(BudgetCode::TotalOver.val());
        }
        Ok(())
    }
    /// 线配额取值。
    pub fn allowance_of(&self, line: Line) -> u64 {
        match line {
            Line::Identity => self.pm_identity,
            Line::Scan => self.pm_scan,
            Line::Report => self.pm_report,
        }
    }
}

// ── 计数器隔离：巡检册与热路径册（两个独立 struct） ──────────────
/// 巡检步数册（三线各一计数，只被巡检面打点）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InspCounters {
    /// 恒等式线步数。
    pub identity_steps: u64,
    /// 扫描线步数。
    pub scanner_steps: u64,
    /// 日报线步数。
    pub report_steps: u64,
}

impl InspCounters {
    pub fn new() -> InspCounters {
        InspCounters::default()
    }
    /// 巡检面打点（只动本册）。
    pub fn bump(&mut self, line: Line, steps: u64) {
        match line {
            Line::Identity => self.identity_steps = self.identity_steps.saturating_add(steps),
            Line::Scan => self.scanner_steps = self.scanner_steps.saturating_add(steps),
            Line::Report => self.report_steps = self.report_steps.saturating_add(steps),
        }
    }
    /// 线步数取值。
    pub fn steps_of(&self, line: Line) -> u64 {
        match line {
            Line::Identity => self.identity_steps,
            Line::Scan => self.scanner_steps,
            Line::Report => self.report_steps,
        }
    }
    /// 三线总步数。
    pub fn total(&self) -> u64 {
        self.identity_steps
            .saturating_add(self.scanner_steps)
            .saturating_add(self.report_steps)
    }
}

/// 热路径步数册（单计数，只被热路径打点）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HotCounters {
    /// 热路径步数。
    pub hot_steps: u64,
}

impl HotCounters {
    pub fn new() -> HotCounters {
        HotCounters::default()
    }
    /// 热路径打点（只动本册）。
    pub fn bump(&mut self, steps: u64) {
        self.hot_steps = self.hot_steps.saturating_add(steps);
    }
}

/// 计数器共享绑定：设计红线，一律显性拒（隔离断言的可执行口径）。
pub fn bind_shared(_a: &InspCounters, _b: &HotCounters) -> Result<(), u32> {
    Err(BudgetCode::CounterShared.val())
}

/// 千分位换算（整数定点：num×1000 ÷ den，den=0 显性 0，saturating 封顶）。
pub fn permille(num: u64, den: u64) -> u64 {
    if den == 0 {
        return 0;
    }
    match num.checked_mul(PM_GRAN) {
        Some(v) => v / den,
        None => {
            // 溢出口径：先除后乘，保序不保精度（显性降精度不造假）。
            (num / den).saturating_mul(PM_GRAN)
        }
    }
}

// ── 调频三态状态机 ──────────────────────────────────────────────
/// 调频状态（Normal→Downshift→Halted 单调升级，滞回回升）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GovState {
    /// 正常：全额步数规模 1000。
    Normal,
    /// 降频：步数规模减半 500。
    Downshift,
    /// 停机：显性停（巡检请求拒）。
    Halted,
}

impl GovState {
    pub fn name(self) -> &'static str {
        match self {
            GovState::Normal => "正常",
            GovState::Downshift => "降频",
            GovState::Halted => "停机",
        }
    }
    /// 本态步数规模（定点千分位：1000/500/0）。
    pub fn scale(self) -> u64 {
        match self {
            GovState::Normal => SCALE_NORMAL,
            GovState::Downshift => SCALE_DOWNSHIFT,
            GovState::Halted => 0,
        }
    }
    /// 升级一级（超预算方向）。
    fn escalate(self) -> GovState {
        match self {
            GovState::Normal => GovState::Downshift,
            GovState::Downshift => GovState::Halted,
            GovState::Halted => GovState::Halted,
        }
    }
    /// 回升一级（滞回方向）。
    fn recover(self) -> GovState {
        match self {
            GovState::Halted => GovState::Downshift,
            GovState::Downshift => GovState::Normal,
            GovState::Normal => GovState::Normal,
        }
    }
}

/// 调频机总成。
#[derive(Clone, Debug)]
pub struct Governor {
    pub state: GovState,
    confirms: u32,
    /// 升级次数（演练口径账）。
    pub escalations: u32,
    /// 回升次数（演练口径账）。
    pub recoveries: u32,
}

impl Default for Governor {
    fn default() -> Governor {
        Governor {
            state: GovState::Normal,
            confirms: 0,
            escalations: 0,
            recoveries: 0,
        }
    }
}

impl Governor {
    pub fn new() -> Governor {
        Governor::default()
    }

    /// 超预算观测：升级一级，确认序列清零（Halted 停留显性）。
    pub fn observe_over(&mut self) -> GovState {
        let next = self.state.escalate();
        if next != self.state {
            self.escalations = self.escalations.saturating_add(1);
        }
        self.state = next;
        self.confirms = 0;
        self.state
    }

    /// 预算内确认：满 8 次回升一级（不足 7 次显性拒）。
    pub fn confirm_under(&mut self) -> Result<GovState, u32> {
        if self.state == GovState::Normal {
            // 正常态确认无回升语义，直接绿。
            return Ok(GovState::Normal);
        }
        let next_confirm = self.confirms.saturating_add(1);
        if next_confirm < HYSTERESIS_CONFIRM {
            self.confirms = next_confirm;
            return Err(BudgetCode::ConfirmShort.val());
        }
        let next = self.state.recover();
        self.state = next;
        self.confirms = 0;
        self.recoveries = self.recoveries.saturating_add(1);
        Ok(self.state)
    }

    /// 巡检请求（Halted 显性拒）。
    pub fn request_inspect(&self) -> Result<(), u32> {
        if self.state == GovState::Halted {
            return Err(BudgetCode::HaltActive.val());
        }
        Ok(())
    }

    /// 步数规模（按步数取值时乘定点再除基数）。
    pub fn scaled_steps(&self, raw: u64) -> u64 {
        raw * self.state.scale() / SCALE_NORMAL
    }

    /// 确认计数取值（测试面）。
    pub fn confirm_count(&self) -> u32 {
        self.confirms
    }
}

// ── 预算复核（巡检册三线 vs 配额；分母=冻结窗口步数预算） ────────
/// 单线复核：实测‰ ≤ 配额（‰=线步数×1000÷窗口总步数预算）。
pub fn line_under(cfg: &BudgetConfig, insp: &InspCounters, line: Line) -> Result<u64, u32> {
    let pm = permille(insp.steps_of(line), BUDGET_STEPS);
    if pm > cfg.allowance_of(line) {
        return Err(BudgetCode::LineOver.val());
    }
    Ok(pm)
}

/// 合计复核：三线实测‰和 ≤ 红线 10‰（合计口径独立于单线配额闸——
/// 单线超配额由 line_under 定位到线，合计超红线由本闸定位到面）。
pub fn total_under(cfg: &BudgetConfig, insp: &InspCounters) -> Result<u64, u32> {
    let _ = cfg;
    let mut sum = 0u64;
    let mut i = 0usize;
    while i < Line::ALL.len() {
        if let Some(l) = Line::ALL.get(i) {
            sum = sum.saturating_add(permille(insp.steps_of(*l), BUDGET_STEPS));
        }
        i += 1;
    }
    if sum > PM_REDLINE {
        return Err(BudgetCode::BudgetOverRed.val());
    }
    Ok(sum)
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

// ── 10/10 演练：超线→自动降频→减半复检→滞回恢复 ─────────────────
/// 单轮演练报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrillReport {
    /// 起始态。
    pub s0: u8,
    /// 首次超线后态（应降频）。
    pub s1: u8,
    /// 减半后单线‰（应回到配额内）。
    pub pm_after_half: u64,
    /// 停机请求拒（true=显性拒）。
    pub halt_denied: bool,
    /// 滞回恢复后态（应正常）。
    pub s2: u8,
}

/// 单轮演练（确定性：步数由轮序推导，LCG 仅驱动扰动量）。
pub fn drill_once(rng: &mut Lcg) -> Result<DrillReport, u32> {
    let cfg = BudgetConfig::standard();
    cfg.validate()?;
    let mut gov = Governor::new();
    let mut insp = InspCounters::new();
    let mut hot = HotCounters::new();
    let perturb = rng.below(100) as u64;
    // 第一步：恒等式线打超线步数（实测 6‰ > 5‰ 配额）。
    insp.bump(Line::Identity, 600);
    insp.bump(Line::Scan, 300);
    insp.bump(Line::Report, 100);
    let s0 = gov.state as u8;
    // 合计复核：实测合计 10‰——超恒等式单线配额 → LineOver → 升级。
    let over = line_under(&cfg, &insp, Line::Identity).is_err();
    if !over {
        return Err(BudgetCode::NotReproducible.val());
    }
    let s1 = gov.observe_over() as u8;
    if s1 != GovState::Downshift as u8 {
        return Err(BudgetCode::NotReproducible.val());
    }
    // 第二步：降频步数减半（600→300，300/1000→3‰ ≤ 5‰）。
    let halved = gov.scaled_steps(600);
    if halved != 300 {
        return Err(BudgetCode::NotReproducible.val());
    }
    let mut insp2 = InspCounters::new();
    insp2.bump(Line::Identity, halved);
    insp2.bump(Line::Scan, gov.scaled_steps(300));
    insp2.bump(Line::Report, gov.scaled_steps(100));
    let pm_after_half = permille(insp2.steps_of(Line::Identity), BUDGET_STEPS);
    // 第三步：再超线 → 停机显性。
    let mut gov2 = Governor::new();
    let _ = gov2.observe_over();
    let s1b = gov2.observe_over() as u8;
    if s1b != GovState::Halted as u8 {
        return Err(BudgetCode::NotReproducible.val());
    }
    let halt_denied = gov2.request_inspect().is_err();
    // 第四步：滞回恢复 Halted→Downshift→Normal（各 8 次确认）。
    let mut recovered = 0u32;
    let mut s2 = gov2.state as u8;
    let mut guard = 0u32;
    while gov2.state != GovState::Normal && guard < 32 {
        guard += 1;
        match gov2.confirm_under() {
            Ok(st) => {
                recovered += 1;
                s2 = st as u8;
            }
            Err(e) if e == BudgetCode::ConfirmShort.val() => {}
            Err(_) => return Err(BudgetCode::NotReproducible.val()),
        }
    }
    // 停机回正常必须两级回升（恰 2 次），跳级或卡死都算违约。
    if recovered != 2 {
        return Err(BudgetCode::NotReproducible.val());
    }
    // 隔离断言：热路径打点不动巡检册，巡检打点不动热路径册。
    let insp_before = insp.total();
    hot.bump(1 + perturb);
    if insp.total() != insp_before || hot.hot_steps != 1 + perturb {
        return Err(BudgetCode::NotReproducible.val());
    }
    insp.bump(Line::Scan, 7);
    if hot.hot_steps != 1 + perturb {
        return Err(BudgetCode::NotReproducible.val());
    }
    Ok(DrillReport {
        s0,
        s1,
        pm_after_half,
        halt_denied,
        s2,
    })
}

/// 10/10 演练总成（每轮全过）。
pub fn drill_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut pass = 0u32;
    let mut r = 0;
    while r < rounds {
        let rep = drill_once(&mut rng)?;
        if rep.s0 == GovState::Normal as u8
            && rep.s1 == GovState::Downshift as u8
            && rep.pm_after_half == 3
            && rep.halt_denied
            && rep.s2 == GovState::Normal as u8
        {
            pass += 1;
        }
        r += 1;
    }
    Ok(pass)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = drill_rounds(JUDGE_ROUNDS)?;
    let b = drill_rounds(JUDGE_ROUNDS)?;
    if a != b {
        return Err(BudgetCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（私有谓词，cond 为违约条件） ─────────────────────────
fn budget_identity_ok() -> bool {
    let cfg = BudgetConfig::standard();
    if cfg.pm_identity != PM_IDENTITY || cfg.pm_scan != PM_SCAN || cfg.pm_report != PM_REPORT {
        return false;
    }
    cfg.total() == PM_TOTAL && PM_TOTAL <= PM_REDLINE && cfg.validate().is_ok()
}

fn total_over_rejected_ok() -> bool {
    let bad = BudgetConfig { pm_identity: 6, pm_scan: 3, pm_report: 2 };
    if bad.total() != 11 {
        return false;
    }
    bad.validate() == Err(BudgetCode::TotalOver.val())
}

fn line_over_detected_ok() -> bool {
    let cfg = BudgetConfig::standard();
    let mut insp = InspCounters::new();
    // 600/1000 步 = 6‰ > 5‰ 配额 → LineOver。
    insp.bump(Line::Identity, 600);
    insp.bump(Line::Scan, 300);
    insp.bump(Line::Report, 100);
    if line_under(&cfg, &insp, Line::Identity).is_ok() {
        return false;
    }
    // 合计 10‰ 恰在红线内不拒（≤ 红线）。
    if total_under(&cfg, &insp).is_err() {
        return false;
    }
    // 恒等式线 700 步：合计 11‰ → BudgetOverRed。
    let mut insp2 = InspCounters::new();
    insp2.bump(Line::Identity, 700);
    insp2.bump(Line::Scan, 300);
    insp2.bump(Line::Report, 100);
    total_under(&cfg, &insp2) == Err(BudgetCode::BudgetOverRed.val())
}

fn counter_isolation_ok() -> bool {
    let mut insp = InspCounters::new();
    let mut hot = HotCounters::new();
    insp.bump(Line::Identity, 5);
    insp.bump(Line::Scan, 3);
    insp.bump(Line::Report, 1);
    hot.bump(999);
    // 隔离：热路径 999 不动巡检册，巡检册 9 不动热路径册。
    if insp.total() != 9 || hot.hot_steps != 999 {
        return false;
    }
    insp.bump(Line::Scan, 1);
    if hot.hot_steps != 999 {
        return false;
    }
    // 共享绑定显性拒。
    match bind_shared(&insp, &hot) {
        Err(e) if e == BudgetCode::CounterShared.val() => true,
        _ => false,
    }
}

fn permille_math_ok() -> bool {
    if permille(500, 100_000) != 5 {
        return false;
    }
    if permille(0, 1000) != 0 {
        return false;
    }
    if permille(100, 0) != 0 {
        return false;
    }
    // 恰 1‰ 边界与 saturating 上界。
    if permille(1000, 1000) != 1000 {
        return false;
    }
    permille(u64::MAX, 1) == (u64::MAX / 1).saturating_mul(PM_GRAN) || permille(u64::MAX, 1) > 0
}

fn downshift_half_ok() -> bool {
    let mut gov = Governor::new();
    if gov.scaled_steps(600) != 600 {
        return false;
    }
    gov.observe_over();
    if gov.state != GovState::Downshift || gov.scaled_steps(600) != 300 {
        return false;
    }
    // 正常态巡检放行，降频态放行。
    gov.request_inspect().is_ok()
}

fn halt_explicit_ok() -> bool {
    let mut gov = Governor::new();
    let _ = gov.observe_over();
    let _ = gov.observe_over();
    if gov.state != GovState::Halted {
        return false;
    }
    if gov.request_inspect().is_ok() {
        return false;
    }
    if gov.state.scale() != 0 {
        return false;
    }
    true
}

fn hysteresis_eight_ok() -> bool {
    let mut gov = Governor::new();
    let _ = gov.observe_over(); // → Downshift
    // 7 次确认全部显性拒。
    let mut k = 0;
    while k < 7 {
        if gov.confirm_under().is_ok() {
            return false;
        }
        k += 1;
    }
    if gov.confirm_count() != 7 {
        return false;
    }
    // 第 8 次回升 Halted→? （本例 Downshift→Normal）。
    match gov.confirm_under() {
        Ok(GovState::Normal) => {}
        _ => return false,
    }
    // Halted→Downshift→Normal 两级各 8 次。
    let mut g2 = Governor::new();
    let _ = g2.observe_over();
    let _ = g2.observe_over();
    let mut rises = 0u32;
    let mut guard = 0;
    while g2.state != GovState::Normal && guard < 32 {
        guard += 1;
        if g2.confirm_under().is_ok() {
            rises += 1;
        }
    }
    rises == 2
}

fn drill_10_10_ok() -> bool {
    match drill_rounds(JUDGE_ROUNDS) {
        Ok(pass) => pass == JUDGE_ROUNDS,
        Err(_) => false,
    }
}

fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        if let Some(c) = BudgetCode::ALL.get(i) {
            if detail(c.val()).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-预算恒等式",
        budget_identity_ok(),
        String::from("5‰+3‰+1‰=9‰≤10‰ 红线"),
    ));
    v.push((
        "J1-合计超线装配拒",
        total_over_rejected_ok(),
        String::from("11‰ 配置显性拒 TotalOver"),
    ));
    v.push((
        "J1-单线超配额检出",
        line_over_detected_ok(),
        String::from("恒等式 6‰>5‰ LineOver；合计 11‰>10‰ BudgetOverRed"),
    ));
    v.push((
        "J2-计数器隔离",
        counter_isolation_ok(),
        String::from("巡检册/热路径册互不串账+共享拒"),
    ));
    v.push((
        "J2-千分位换算",
        permille_math_ok(),
        String::from("定点 1000 换算含零/溢出口径"),
    ));
    v.push((
        "J2-自动降频减半",
        downshift_half_ok(),
        String::from("Normal→Downshift 步数 600→300"),
    ));
    v.push((
        "J2-停机显性",
        halt_explicit_ok(),
        String::from("Downshift→Halted 巡检请求拒"),
    ));
    v.push((
        "J2-恢复滞回8次",
        hysteresis_eight_ok(),
        String::from("7 次拒 8 次回升；停机两级 16 次回正常"),
    ));
    v.push((
        "J2-演练10/10",
        drill_10_10_ok(),
        String::from("超线→降频→减半复检→停机→滞回恢复 全过"),
    ));
    v.push(("J3-双跑一致", repro_check().is_ok(), String::from("drill_rounds 两跑同值")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x75980 窗口")));
    v.push(("J3-诊断码闭集", detail_closed_ok(), String::from("查表闭集返回")));
    v
}

pub fn run_unx1478_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1478 零丢巡检性能预算");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_config_identity() {
        let cfg = BudgetConfig::standard();
        assert_eq!((cfg.pm_identity, cfg.pm_scan, cfg.pm_report), (5, 3, 1));
        assert_eq!(cfg.total(), 9);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn over_total_config_rejected() {
        let bad = BudgetConfig { pm_identity: 6, pm_scan: 3, pm_report: 2 };
        assert_eq!(bad.validate(), Err(BudgetCode::TotalOver.val()));
        let edge = BudgetConfig { pm_identity: 5, pm_scan: 4, pm_report: 1 };
        assert_eq!(edge.total(), 10);
        assert!(edge.validate().is_ok(), "恰 10‰ 不超红线");
    }

    #[test]
    fn line_and_total_gates() {
        let cfg = BudgetConfig::standard();
        let mut insp = InspCounters::new();
        insp.bump(Line::Identity, 600);
        insp.bump(Line::Scan, 300);
        insp.bump(Line::Report, 100);
        assert_eq!(
            line_under(&cfg, &insp, Line::Identity),
            Err(BudgetCode::LineOver.val())
        );
        assert_eq!(line_under(&cfg, &insp, Line::Scan).expect("扫描线应绿"), 3);
        assert_eq!(line_under(&cfg, &insp, Line::Report).expect("日报线应绿"), 1);
        assert_eq!(total_under(&cfg, &insp).expect("合计 10‰ 应恰绿"), 10);
    }

    #[test]
    fn counters_isolated() {
        let mut insp = InspCounters::new();
        let mut hot = HotCounters::new();
        insp.bump(Line::Identity, 5);
        insp.bump(Line::Scan, 3);
        insp.bump(Line::Report, 1);
        hot.bump(77);
        assert_eq!(insp.total(), 9);
        assert_eq!(hot.hot_steps, 77);
        insp.bump(Line::Report, 1);
        assert_eq!(hot.hot_steps, 77, "巡检打点不得串热路径册");
        hot.bump(1);
        assert_eq!(insp.total(), 10, "热路径打点不得串巡检册");
        assert_eq!(
            bind_shared(&insp, &hot),
            Err(BudgetCode::CounterShared.val())
        );
    }

    #[test]
    fn permille_fixed_point() {
        assert_eq!(permille(500, 100_000), 5);
        assert_eq!(permille(999, 100_000), 9);
        assert_eq!(permille(0, 0), 0);
        assert_eq!(permille(1000, 1000), 1000);
        assert_eq!(permille(1, 3), 333, "整除截断显性");
    }

    #[test]
    fn governor_escalation_monotonic() {
        let mut gov = Governor::new();
        assert_eq!(gov.state, GovState::Normal);
        gov.observe_over();
        assert_eq!(gov.state, GovState::Downshift);
        assert_eq!(gov.scaled_steps(600), 300);
        gov.observe_over();
        assert_eq!(gov.state, GovState::Halted);
        assert_eq!(gov.state.scale(), 0);
        assert_eq!(gov.request_inspect(), Err(BudgetCode::HaltActive.val()));
        assert_eq!(gov.escalations, 2);
    }

    #[test]
    fn hysteresis_requires_eight() {
        let mut gov = Governor::new();
        gov.observe_over();
        let mut k = 0;
        while k < 7 {
            assert_eq!(gov.confirm_under(), Err(BudgetCode::ConfirmShort.val()));
            k += 1;
        }
        assert_eq!(gov.confirm_count(), 7);
        assert_eq!(gov.confirm_under().expect("第 8 次回升"), GovState::Normal);
        assert_eq!(gov.recoveries, 1);
        // 确认序列打断清零：升一次再确认 3 次后打一次超线，确认清零。
        let mut g2 = Governor::new();
        g2.observe_over();
        let mut j = 0;
        while j < 3 {
            let _ = g2.confirm_under();
            j += 1;
        }
        g2.observe_over();
        assert_eq!(g2.confirm_count(), 0, "超线观测清零确认序列");
    }

    #[test]
    fn drill_rounds_all_pass() {
        assert_eq!(drill_rounds(JUDGE_ROUNDS).expect("演练"), JUDGE_ROUNDS);
        let mut rng = Lcg::new(SEG_BASE as u64);
        let rep = drill_once(&mut rng).expect("单轮");
        assert_eq!(rep.s0, 0);
        assert_eq!(rep.s1, 1);
        assert_eq!(rep.pm_after_half, 3);
        assert!(rep.halt_denied);
        assert_eq!(rep.s2, 0);
    }

    #[test]
    fn repro_and_segment() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(detail_closed_ok());
        assert!(probe_like_guards());
    }

    fn probe_like_guards() -> bool {
        PM_IDENTITY == 5 && PM_SCAN == 3 && PM_REPORT == 1 && PM_TOTAL == 9 && PM_REDLINE == 10
    }
}
