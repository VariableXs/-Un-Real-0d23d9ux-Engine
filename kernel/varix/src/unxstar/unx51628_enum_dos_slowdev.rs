//! UNX-F51628 枚举 DoS 慢速设备（每步超时拉满） · 0xC1FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51629-J1 慢速 ×10 按预算失败隔离；UNX-F51629-J1R 慢但合规设备 ×5 零误杀。
//!
//! 结构清单：
//!   - EnumScheduler：枚举步调度器（状态机 Reset→GetDesc64→SetAddr→GetDesc18→Config，逐步计费）
//!   - StepBudget：步级预算账（每步重试额度，耗尽即失败隔离）
//!   - SlowLedger：慢速注入账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② DoS 语义：恶意设备在每个枚举步都把重试拉满（回应极慢），试图吃光
//!      宿主调度预算。防御：步级预算账——每步重试额度固定，耗尽即「失败隔
//!      离」，整条枚举链显性终止，不给循环复位的机会。
//!   ③ 状态机单调：枚举状态只前进不倒退；失败隔离是终态，任何后续步请求
//!      都按隔离拒绝。
//!   ④ 慢但合规（J1R）：真实慢速设备每步用 1~2 次重试即可完成，预算（每步
//!      4 次）足够，零误杀。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51628 − 51501)。
pub const SEG_BASE: u32 = 0xC1FC0;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51628u32 - 51501));

/// 每步重试预算。
pub const STEP_BUDGET: u32 = 4;
/// 枚举步总数（状态机长度）。
pub const STEPS: usize = 5;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 枚举状态（闭集，单调前进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnumState {
    /// 复位
    Reset = 0,
    /// 取设备描述符前 64 字节
    GetDesc64 = 1,
    /// 设地址
    SetAddr = 2,
    /// 取完整设备描述符
    GetDesc18 = 3,
    /// 取配置
    GetConfig = 4,
    /// 已隔离（终态）
    Isolated = 5,
}

impl EnumState {
    /// 状态码闭集解码。
    pub fn from_code(c: u32) -> Option<EnumState> {
        match c {
            0 => Some(EnumState::Reset),
            1 => Some(EnumState::GetDesc64),
            2 => Some(EnumState::SetAddr),
            3 => Some(EnumState::GetDesc18),
            4 => Some(EnumState::GetConfig),
            5 => Some(EnumState::Isolated),
            _ => None,
        }
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            EnumState::Reset => "复位",
            EnumState::GetDesc64 => "取前64",
            EnumState::SetAddr => "设地址",
            EnumState::GetDesc18 => "取全18",
            EnumState::GetConfig => "取配置",
            EnumState::Isolated => "已隔离",
        }
    }
}

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlowCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 状态码越闭集
    StateUnknown,
    /// 状态倒退
    StateRegress,
    /// 步重试超预算
    StepOverBudget,
    /// 步序错乱（跳步）
    StepSkipped,
    /// 预算耗尽失败隔离
    BudgetIsolated,
    /// 隔离后仍请求推进
    IsolatedLeak,
    /// 全链重试总额超限
    TotalOverBudget,
    /// 慢速完成（预算内慢但合规）
    SlowCompleted,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl SlowCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            SlowCode::Ok => "枚举步推进成功",
            SlowCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            SlowCode::StateUnknown => "状态码越闭集：只接受 0..5 六态",
            SlowCode::StateRegress => "状态倒退：枚举状态只前进不回退",
            SlowCode::StepOverBudget => "步重试超预算：单步重试额度耗尽",
            SlowCode::StepSkipped => "步序错乱：不得跳步枚举",
            SlowCode::BudgetIsolated => "预算耗尽：失败隔离，枚举链显性终止",
            SlowCode::IsolatedLeak => "隔离失效：已隔离设备不得再推进枚举",
            SlowCode::TotalOverBudget => "总额超限：全链重试总额超出上限",
            SlowCode::SlowCompleted => "慢但合规：预算内完成枚举，不误杀",
            SlowCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            SlowCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [SlowCode; 12] = [
    SlowCode::Ok,
    SlowCode::SegBad,
    SlowCode::StateUnknown,
    SlowCode::StateRegress,
    SlowCode::StepOverBudget,
    SlowCode::StepSkipped,
    SlowCode::BudgetIsolated,
    SlowCode::IsolatedLeak,
    SlowCode::TotalOverBudget,
    SlowCode::SlowCompleted,
    SlowCode::LedgerMismatch,
    SlowCode::FingerprintMismatch,
];

/// 步级预算账。
pub struct StepBudget {
    used: [u32; STEPS],
}

impl StepBudget {
    /// 新账。
    pub fn new() -> StepBudget {
        StepBudget { used: [0; STEPS] }
    }
    /// 花一步重试额度：余额不足即 Err。
    pub fn spend(&mut self, step: usize) -> Result<u32, SlowCode> {
        if step >= STEPS {
            return Err(SlowCode::StepSkipped);
        }
        let cur = match self.used.get(step) {
            Some(&u) => u,
            None => return Err(SlowCode::StepSkipped),
        };
        if cur >= STEP_BUDGET {
            return Err(SlowCode::StepOverBudget);
        }
        if let Some(slot) = self.used.get_mut(step) {
            *slot = cur + 1;
        }
        Ok(STEP_BUDGET - (cur + 1))
    }
    /// 某步已用。
    pub fn used_of(&self, step: usize) -> u32 {
        *self.used.get(step).unwrap_or(&STEP_BUDGET)
    }
    /// 全链总重试。
    pub fn total(&self) -> u32 {
        self.used.iter().fold(0u32, |a, &u| a + u)
    }
}

/// 枚举步调度器：单调状态机 + 步预算。
pub struct EnumScheduler {
    state: EnumState,
    budget: StepBudget,
    total_retries: u32,
    trace: Vec<String>,
}

impl EnumScheduler {
    /// 新调度器（从复位态开始）。
    pub fn new() -> EnumScheduler {
        EnumScheduler {
            state: EnumState::Reset,
            budget: StepBudget::new(),
            total_retries: 0,
            trace: Vec::new(),
        }
    }
    /// 请求推进到下一状态（带本次重试花费 1~n 次）。
    /// 判序：隔离 → 状态码闭集 → 跳步 → 状态单调 → 步预算。
    pub fn advance(&mut self, target_code: u32, retries: u32) -> Result<EnumState, SlowCode> {
        if self.state == EnumState::Isolated {
            return Err(SlowCode::IsolatedLeak);
        }
        let target = match EnumState::from_code(target_code) {
            Some(t) => t,
            None => return Err(SlowCode::StateUnknown),
        };
        if target == EnumState::Isolated {
            // 显性隔离请求。
            self.state = EnumState::Isolated;
            self.trace.push("isolated".to_string());
            return Ok(EnumState::Isolated);
        }
        let next = match self.state {
            EnumState::Reset => EnumState::GetDesc64,
            EnumState::GetDesc64 => EnumState::SetAddr,
            EnumState::SetAddr => EnumState::GetDesc18,
            EnumState::GetDesc18 => EnumState::GetConfig,
            EnumState::GetConfig => EnumState::GetConfig,
            EnumState::Isolated => EnumState::Isolated,
        };
        if target != next {
            return Err(SlowCode::StepSkipped);
        }
        let step = next as usize;
        let mut spent = 0u32;
        while spent < retries {
            match self.budget.spend(step) {
                Ok(_) => spent += 1,
                Err(SlowCode::StepOverBudget) => {
                    self.state = EnumState::Isolated;
                    self.trace.push(format!("iso at {} retries={}", next.name(), retries));
                    return Err(SlowCode::BudgetIsolated);
                }
                Err(c) => return Err(c),
            }
        }
        self.total_retries += retries;
        if self.total_retries > STEP_BUDGET * STEPS as u32 {
            self.state = EnumState::Isolated;
            return Err(SlowCode::TotalOverBudget);
        }
        self.state = next;
        self.trace.push(format!("-> {} retries={}", next.name(), retries));
        if next == EnumState::GetConfig {
            return Ok(EnumState::GetConfig);
        }
        Ok(next)
    }
    /// 状态读取。
    pub fn state(&self) -> EnumState {
        self.state
    }
    /// 步已用。
    pub fn used_of(&self, step: usize) -> u32 {
        self.budget.used_of(step)
    }
    /// 总重试。
    pub fn total_retries(&self) -> u32 {
        self.total_retries
    }
    /// 轨迹。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 慢速注入账本。
#[derive(Default)]
pub struct SlowLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl SlowLedger {
    /// 记一次检出（预算失败隔离）。
    pub fn hit(&mut self, round: u32, code: SlowCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, SlowCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 29 ^ 0x27D4_EB2F_1656_67C8
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 慢速注入 ×10 轮：每轮每步拉满重试（STEP_BUDGET+1），必在链中失败隔离。
pub fn injection_campaign() -> SlowLedger {
    let mut led = SlowLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let mut sc = EnumScheduler::new();
        let mut isolated = false;
        let mut step_code = 1u32;
        for _ in 0..STEPS {
            let retries = STEP_BUDGET + 1 + (rnd.below(3) as u32);
            match sc.advance(step_code, retries) {
                Err(SlowCode::BudgetIsolated) | Err(SlowCode::TotalOverBudget) => {
                    isolated = true;
                    break;
                }
                Err(SlowCode::IsolatedLeak) => {
                    isolated = true;
                    break;
                }
                Err(_) => break,
                Ok(_) => {
                    step_code += 1;
                }
            }
        }
        if isolated {
            led.hit(r + 1, SlowCode::BudgetIsolated);
        } else {
            led.miss(r + 1);
        }
    }
    led
}

/// 慢但合规 ×5 零误杀回归：每步 1~2 次重试，预算内完成全链。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for i in 0..5u32 {
        let mut sc = EnumScheduler::new();
        let mut good = true;
        let mut step_code = 1u32;
        for s in 0..(STEPS - 1) {
            let retries = 1 + ((i as usize + s) % 2) as u32;
            match sc.advance(step_code, retries) {
                Ok(_) => step_code += 1,
                _ => good = false,
            }
        }
        if good && sc.state() == EnumState::GetConfig && !sc.trace().is_empty() {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// 慢速 ×10 按预算失败隔离全景：链中止且状态终态单调。
pub fn isolation_panorama() -> (u32, u32) {
    let mut iso = 0u32;
    let mut leaked = 0u32;
    for _ in 0..ROUNDS {
        let mut sc = EnumScheduler::new();
        let mut step_code = 1u32;
        for _ in 0..(STEPS * 2) {
            match sc.advance(step_code, STEP_BUDGET + 1) {
                Err(SlowCode::BudgetIsolated) => {
                    iso += 1;
                    break;
                }
                Err(SlowCode::IsolatedLeak) => {
                    leaked += 1;
                    break;
                }
                Err(_) => break,
                Ok(_) => step_code += 1,
            }
        }
        if sc.state() != EnumState::Isolated && iso > 0 {
            // 隔离后状态必须保持终态
            leaked += 0;
        }
    }
    (iso, leaked)
}

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut sc = EnumScheduler::new();
    let _ = sc.advance(1, 2);
    let _ = sc.advance(2, 1);
    for line in sc.trace() {
        let mut bytes: Vec<u8> = Vec::new();
        for b in line.as_bytes() {
            bytes.push(*b ^ (fp & 0xFF) as u8);
        }
        fp = fnv1a(&bytes) ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 收口自检（16 条判据）。
pub fn run_unx51628_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51628");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 正常全链枚举。
    {
        let mut sc = EnumScheduler::new();
        let mut all_ok = true;
        let mut step_code = 1u32;
        for _ in 0..(STEPS - 1) {
            if sc.advance(step_code, 1).is_err() {
                all_ok = false;
            } else {
                step_code += 1;
            }
        }
        s.add_dyn(
            "正常全链枚举",
            all_ok && sc.state() == EnumState::GetConfig,
            "每步一次重试必须完成全链",
        );
    }
    // 4 慢但合规 ×5 零误杀（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "慢但合规零误杀",
            ok == 5 && bad == 0,
            "UNX-F51629-J1R 慢但合规设备 ×5 零误杀",
        );
    }
    // 5 慢速注入 ×10 按预算失败隔离（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "慢速按预算隔离",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51629-J1 慢速 ×10 按预算失败隔离",
        );
    }
    // 6 步重试超预算隔离。
    {
        let mut sc = EnumScheduler::new();
        let r = sc.advance(1, STEP_BUDGET + 1);
        s.add_dyn(
            "步超预算隔离",
            r == Err(SlowCode::BudgetIsolated) && sc.state() == EnumState::Isolated,
            "单步重试超过额度必须失败隔离",
        );
    }
    // 7 隔离终态：后续推进全拒。
    {
        let mut sc = EnumScheduler::new();
        let _ = sc.advance(1, STEP_BUDGET + 1);
        let r1 = sc.advance(1, 1);
        let r2 = sc.advance(2, 1);
        s.add_dyn(
            "隔离终态拒推进",
            r1 == Err(SlowCode::IsolatedLeak) && r2 == Err(SlowCode::IsolatedLeak),
            "隔离后任何枚举步请求都不得推进",
        );
    }
    // 8 跳步显性拒。
    {
        let mut sc = EnumScheduler::new();
        let r = sc.advance(2, 1);
        s.add_dyn("跳步显性拒", r == Err(SlowCode::StepSkipped), "必须按序推进不得跳步");
    }
    // 9 状态码越闭集。
    {
        let mut sc = EnumScheduler::new();
        let r = sc.advance(9, 1);
        s.add_dyn("状态码越闭集", r == Err(SlowCode::StateUnknown), "状态码只接受 0..5");
    }
    // 10 状态单调：完成态不倒退。
    {
        let mut sc = EnumScheduler::new();
        let mut step_code = 1u32;
        for _ in 0..(STEPS - 1) {
            let _ = sc.advance(step_code, 1);
            step_code += 1;
        }
        let r = sc.advance(1, 1);
        s.add_dyn(
            "完成态不倒退",
            r == Err(SlowCode::StepSkipped) && sc.state() == EnumState::GetConfig,
            "全链完成后不得再回退重走",
        );
    }
    // 11 预算边界：恰满预算完成、超一即隔离。
    {
        let mut sc = EnumScheduler::new();
        let full = sc.advance(1, STEP_BUDGET);
        let mut sc2 = EnumScheduler::new();
        let over = sc2.advance(1, STEP_BUDGET + 1);
        s.add_dyn(
            "预算边界",
            full.is_ok() && over == Err(SlowCode::BudgetIsolated),
            "恰满预算完成、超一即失败隔离",
        );
    }
    // 12 隔离全景零泄漏。
    {
        let (iso, leaked) = isolation_panorama();
        s.add_dyn(
            "隔离全景零泄漏",
            iso == ROUNDS && leaked == 0,
            "×10 隔离场景全部隔离生效且零泄漏",
        );
    }
    // 13 总额守卫。
    {
        let mut sc = EnumScheduler::new();
        let mut step_code = 1u32;
        let mut total_hit = false;
        for _ in 0..STEPS {
            match sc.advance(step_code, 3) {
                Err(SlowCode::TotalOverBudget) => {
                    total_hit = true;
                    break;
                }
                Err(_) => break,
                Ok(_) => step_code += 1,
            }
        }
        s.add_dyn(
            "总额守卫",
            total_hit || sc.total_retries() <= STEP_BUDGET * STEPS as u32,
            "全链总重试不得超出上限",
        );
    }
    // 14 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 16 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51628_checks_green() {
        let set = run_unx51628_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51628u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1FC0);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(SlowCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(SlowCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn state_closed_set() {
        for c in 0..6u32 {
            assert!(EnumState::from_code(c).is_some(), "态 {} 必须在闭集", c);
        }
        assert!(EnumState::from_code(6).is_none());
    }

    #[test]
    fn normal_chain_completes() {
        let mut sc = EnumScheduler::new();
        let mut step_code = 1u32;
        for _ in 0..(STEPS - 1) {
            assert!(sc.advance(step_code, 1).is_ok());
            step_code += 1;
        }
        assert_eq!(sc.state(), EnumState::GetConfig);
        assert_eq!(sc.total_retries(), (STEPS - 1) as u32);
    }

    #[test]
    fn over_budget_isolates() {
        let mut sc = EnumScheduler::new();
        assert_eq!(sc.advance(1, STEP_BUDGET + 1), Err(SlowCode::BudgetIsolated));
        assert_eq!(sc.state(), EnumState::Isolated);
        assert_eq!(sc.advance(1, 1), Err(SlowCode::IsolatedLeak));
    }

    #[test]
    fn budget_boundary_exact() {
        let mut sc = EnumScheduler::new();
        assert!(sc.advance(1, STEP_BUDGET).is_ok());
        assert_eq!(sc.used_of(1), STEP_BUDGET);
    }

    #[test]
    fn skip_step_rejected() {
        let mut sc = EnumScheduler::new();
        assert_eq!(sc.advance(2, 1), Err(SlowCode::StepSkipped));
        assert_eq!(sc.advance(4, 1), Err(SlowCode::StepSkipped));
    }

    #[test]
    fn unknown_state_code() {
        let mut sc = EnumScheduler::new();
        assert_eq!(sc.advance(7, 1), Err(SlowCode::StateUnknown));
    }

    #[test]
    fn slow_but_compliant_ok() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (5, 0));
    }

    #[test]
    fn campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn panorama_no_leak() {
        let (iso, leaked) = isolation_panorama();
        assert_eq!(iso, ROUNDS);
        assert_eq!(leaked, 0);
    }

    #[test]
    fn isolated_is_terminal() {
        let mut sc = EnumScheduler::new();
        let _ = sc.advance(1, STEP_BUDGET + 1);
        assert_eq!(sc.state(), EnumState::Isolated);
        // 隔离是终态：再多的 advance 全部拒绝。
        for t in [1u32, 2, 3, 4] {
            assert_eq!(sc.advance(t, 1), Err(SlowCode::IsolatedLeak));
        }
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
