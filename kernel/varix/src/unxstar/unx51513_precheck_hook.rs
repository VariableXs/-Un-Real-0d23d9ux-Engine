//! UNX-F51513 写路径写前校验钩子性能账（红线件开销 P95） · 0xC0340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51514-J1 挂起期命令 ×100 唤醒后按序补投零丢失
//!
//! 设计要点：
//!   ① 写前校验三道闸（冻结表）：分区界校验 / 写保护位 / 数据 CRC，各闸
//!      定格开销为账面常数，红线件总开销 = Σ闸开销 + 抖动余量。
//!   ② 开销千分比落账：每样本记 (基线 ticks, 挂钩后 ticks)，开销‰ =
//!      Δ×1000/基线，全程 checked/saturating 整数域，零浮点。
//!   ③ P95 落账：样本开销‰升序取 95 分位，红线 ≤50‰（即 5%）；越线样本
//!      即时记违约，P95 越线整账翻红。
//!   ④ 钩子状态机 Disabled→Enabled→Enforced 单向晋级，倒退显性拒。
//!   ⑤ 违约注入必抓 ×10：超标开销、负开销（挂钩后 < 基线，账面不守恒）、
//!      状态倒退三类注入全检出。
//!   ⑥ 守恒账：注入 = 检出 + 逃逸，逃逸恒 0；固定种子 LCG 生成确定性
//!      成本样本，双跑逐字段一致。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0340;

/// 基线写操作开销（ticks，冻结表）。
pub const BASELINE_TICKS: u64 = 1_000;
/// 红线开销千分比上限（50‰ = 5%，冻结表）。
pub const BREACH_PERMILLE: u64 = 50;
/// 样本账容量（冻结表）。
pub const SAMPLE_CAP: usize = 64;
/// LCG 种子（冻结，禁随机漂移）。
pub const LCG_SEED: u64 = 0x2A517_2025_51513;

/// 诊断码闭集（写前校验钩子域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrecheckCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 样本账已满
    SampleCap,
    /// 基线为零（分母非法）
    ZeroBaseline,
    /// 负开销（挂钩后 < 基线，账面不守恒）
    NegativeOverhead,
    /// 千分比计算溢出
    PermilleOverflow,
    /// 状态机倒退
    StateRegress,
    /// 空账求分位
    SampleEmpty,
    /// 闸位组合非法
    HookMismatch,
    /// 守恒账不平
    LedgerMismatch,
}

impl PrecheckCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PrecheckCode::Ok => "样本入账成功",
            PrecheckCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            PrecheckCode::SampleCap => "样本账已满：归档后再续账",
            PrecheckCode::ZeroBaseline => "基线为零：分母非法，先测基线再挂钩",
            PrecheckCode::NegativeOverhead => "负开销：挂钩后不得小于基线，账面不守恒",
            PrecheckCode::PermilleOverflow => "千分比溢出：缩短测量窗口或加大基线",
            PrecheckCode::StateRegress => "状态机倒退：钩子只许晋级不许降级",
            PrecheckCode::SampleEmpty => "空账求分位：先落样本再算 P95",
            PrecheckCode::HookMismatch => "闸位组合非法：核对三道闸的使能矩阵",
            PrecheckCode::LedgerMismatch => "守恒账不平：注入=检出+逃逸被破坏",
        }
    }
}

/// 写前校验三道闸（冻结表，定格开销为账面常数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookKind {
    /// 分区界校验
    Boundary,
    /// 写保护位
    WProtect,
    /// 数据 CRC
    Crc,
}

impl HookKind {
    /// 闸位定格开销（ticks，冻结表）。
    pub fn cost(self) -> u64 {
        match self {
            HookKind::Boundary => 12,
            HookKind::WProtect => 8,
            HookKind::Crc => 20,
        }
    }
}

/// 钩子状态机：单向晋级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookState {
    /// 未挂载
    Disabled,
    /// 已挂载只记数
    Enabled,
    /// 强制闸（红线件生效）
    Enforced,
}

/// 单条样本账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRec {
    pub seq: u32,
    pub baseline: u64,
    pub with_hook: u64,
    pub permille: u64,
}

/// 性能账引擎：样本账 + LCG 成本面 + 守恒账。
pub struct PrecheckLedger {
    state: HookState,
    samples: Vec<SampleRec>,
    seed: u64,
    breaches: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl PrecheckLedger {
    pub fn new() -> PrecheckLedger {
        PrecheckLedger {
            state: HookState::Disabled,
            samples: Vec::new(),
            seed: LCG_SEED,
            breaches: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    pub fn state_is(&self, st: HookState) -> bool {
        self.state == st
    }
    pub fn samples_len(&self) -> usize {
        self.samples.len()
    }
    pub fn breaches(&self) -> u64 {
        self.breaches
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 单向晋级：Disabled→Enabled→Enforced，倒退显性拒（注入检出点之一）。
    pub fn promote_to(&mut self, target: HookState) -> Result<(), PrecheckCode> {
        let rank = |s: HookState| -> u8 {
            match s {
                HookState::Disabled => 0,
                HookState::Enabled => 1,
                HookState::Enforced => 2,
            }
        };
        if rank(target) < rank(self.state) {
            return Err(PrecheckCode::StateRegress);
        }
        self.state = target;
        Ok(())
    }

    /// 固定种子 LCG（确定性成本面，禁随机漂移）。
    fn lcg_next(&mut self) -> u64 {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.seed >> 33
    }

    /// 按闸位组合与 LCG 抖动生成一次挂钩后开销（域内规范成本面）。
    pub fn hooked_cost(&mut self, hooks: &[HookKind]) -> Result<u64, PrecheckCode> {
        if hooks.is_empty() {
            return Err(PrecheckCode::HookMismatch);
        }
        let mut sum: u64 = 0;
        for k in hooks.iter() {
            sum = sum.saturating_add(k.cost());
        }
        let jitter = self.lcg_next() % 8;
        Ok(BASELINE_TICKS.saturating_add(sum).saturating_add(jitter))
    }

    /// 样本入账：容量 → 零基线 → 负开销 → 千分比溢出，返回开销‰。
    pub fn record(&mut self, baseline: u64, with_hook: u64) -> Result<u64, PrecheckCode> {
        if self.samples.len() >= SAMPLE_CAP {
            return Err(PrecheckCode::SampleCap);
        }
        if baseline == 0 {
            return Err(PrecheckCode::ZeroBaseline);
        }
        if with_hook < baseline {
            return Err(PrecheckCode::NegativeOverhead);
        }
        let delta = with_hook - baseline;
        let num = match delta.checked_mul(1000) {
            Some(v) => v,
            None => return Err(PrecheckCode::PermilleOverflow),
        };
        let pm = num / baseline;
        let rec = SampleRec {
            seq: self.samples.len() as u32,
            baseline,
            with_hook,
            permille: pm,
        };
        self.samples.push(rec);
        if pm > BREACH_PERMILLE {
            self.breaches += 1;
        }
        Ok(pm)
    }

    /// 样本开销‰全量快照。
    pub fn permilles(&self) -> Vec<u64> {
        self.samples.iter().map(|r| r.permille).collect::<Vec<_>>()
    }

    /// P95 分位：升序插排后取 95 分位锚点（零浮点、域内手写排序）。
    pub fn p95(&self) -> Result<u64, PrecheckCode> {
        if self.samples.is_empty() {
            return Err(PrecheckCode::SampleEmpty);
        }
        let mut arr = self.permilles();
        let n = arr.len();
        for i in 1..n {
            let mut j = i;
            while j > 0 {
                let a = match arr.get(j - 1) {
                    Some(v) => *v,
                    None => break,
                };
                let b = match arr.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                if a > b {
                    arr.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        let idx64 = ((n as u64).saturating_mul(95)) / 100;
        let idx = idx64.min(n as u64 - 1) as usize;
        match arr.get(idx) {
            Some(v) => Ok(*v),
            None => Err(PrecheckCode::SampleEmpty),
        }
    }

    /// P95 红线判定：≤50‰ 视为落账达标。
    pub fn p95_within_budget(&self) -> bool {
        match self.p95() {
            Ok(v) => v <= BREACH_PERMILLE,
            Err(_) => false,
        }
    }

    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }
    /// 守恒面：注入 = 检出 + 逃逸，逃逸恒 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同种子 LCG 两次建账 P95 与样本逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> (Vec<u64>, u64, u64) {
        let mut led = PrecheckLedger::new();
        let _ = led.promote_to(HookState::Enabled);
        let hooks = [HookKind::Boundary, HookKind::WProtect, HookKind::Crc];
        for _ in 0..16u32 {
            let cost = match led.hooked_cost(&hooks) {
                Ok(c) => c,
                Err(_) => BASELINE_TICKS + 40,
            };
            let _ = led.record(BASELINE_TICKS, cost);
        }
        let p = match led.p95() {
            Ok(v) => v,
            Err(_) => 0,
        };
        (led.permilles(), p, led.breaches())
    };
    build() == build()
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [PrecheckCode; 10] = [
    PrecheckCode::Ok,
    PrecheckCode::SegBad,
    PrecheckCode::SampleCap,
    PrecheckCode::ZeroBaseline,
    PrecheckCode::NegativeOverhead,
    PrecheckCode::PermilleOverflow,
    PrecheckCode::StateRegress,
    PrecheckCode::SampleEmpty,
    PrecheckCode::HookMismatch,
    PrecheckCode::LedgerMismatch,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51513_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51513");
    // 1 正常流：三道闸全开 16 样本，P95 落账 ≤5%。
    {
        let mut led = PrecheckLedger::new();
        let _ = led.promote_to(HookState::Enabled);
        let _ = led.promote_to(HookState::Enforced);
        let hooks = [HookKind::Boundary, HookKind::WProtect, HookKind::Crc];
        let mut all_ok = true;
        for _ in 0..16u32 {
            let cost = match led.hooked_cost(&hooks) {
                Ok(c) => c,
                Err(_) => {
                    all_ok = false;
                    BASELINE_TICKS
                }
            };
            if led.record(BASELINE_TICKS, cost).is_err() {
                all_ok = false;
            }
        }
        s.add_dyn(
            "三闸正常流P95落账",
            all_ok && led.p95_within_budget() && led.breaches() == 0,
            "三闸定格开销加抖动必须 P95 ≤ 50‰",
        );
    }
    // 2 边界：开销恰在红线与红线+1。
    {
        let mut led = PrecheckLedger::new();
        let at = led.record(BASELINE_TICKS, BASELINE_TICKS + 50);
        let over = led.record(BASELINE_TICKS, BASELINE_TICKS + 51);
        s.add_dyn(
            "红线等号与+1",
            at == Ok(BREACH_PERMILLE) && led.breaches() == 0 && over == Ok(BREACH_PERMILLE + 1) && led.breaches() == 1,
            "50‰ 落账达标，51‰ 即时记违约",
        );
    }
    // 3 边界：基线为零拒绝。
    {
        let mut led = PrecheckLedger::new();
        let r = led.record(0, 100);
        s.add_dyn("零基线拒", r == Err(PrecheckCode::ZeroBaseline), "分母非法必须明拒");
    }
    // 4 负边界：挂钩后小于基线（负开销）拒绝。
    {
        let mut led = PrecheckLedger::new();
        let r = led.record(BASELINE_TICKS, BASELINE_TICKS - 1);
        s.add_dyn(
            "负开销拒",
            r == Err(PrecheckCode::NegativeOverhead),
            "挂钩后低于基线是账面不守恒，必须明拒",
        );
    }
    // 5 溢出边界：千分比乘法溢出拒绝。
    {
        let mut led = PrecheckLedger::new();
        let r = led.record(1, u64::MAX);
        s.add_dyn(
            "千分比溢出拒",
            r == Err(PrecheckCode::PermilleOverflow),
            "checked_mul 溢出必须显性拒绝",
        );
    }
    // 6 边界：样本账容量与上限+1。
    {
        let mut led = PrecheckLedger::new();
        let mut all_ok = true;
        for _ in 0..SAMPLE_CAP {
            if led.record(BASELINE_TICKS, BASELINE_TICKS + 40).is_err() {
                all_ok = false;
            }
        }
        let over = led.record(BASELINE_TICKS, BASELINE_TICKS + 40);
        s.add_dyn(
            "样本账容量耗尽",
            all_ok && over == Err(PrecheckCode::SampleCap),
            "样本满账后续账必须明拒",
        );
    }
    // 7 注入必抓：超标开销 ×10。
    {
        let mut led = PrecheckLedger::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if let Ok(pm) = led.record(BASELINE_TICKS, BASELINE_TICKS + 200) {
                if pm > BREACH_PERMILLE {
                    caught += 1;
                    led.note_detected();
                }
            }
        }
        s.add_dyn("超标开销 10/10", caught == 10, "红线件开销越线必须全抓");
    }
    // 8 注入必抓：负开销 ×10。
    {
        let mut led = PrecheckLedger::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if led.record(BASELINE_TICKS, BASELINE_TICKS - 1) == Err(PrecheckCode::NegativeOverhead) {
                caught += 1;
                led.note_detected();
            }
        }
        s.add_dyn("负开销 10/10", caught == 10, "负开销注入必须全抓");
    }
    // 9 注入必抓：状态机倒退 ×10。
    {
        let mut led = PrecheckLedger::new();
        let _ = led.promote_to(HookState::Enabled);
        let _ = led.promote_to(HookState::Enforced);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if led.promote_to(HookState::Disabled) == Err(PrecheckCode::StateRegress) {
                caught += 1;
                led.note_detected();
            }
        }
        s.add_dyn("状态倒退 10/10", caught == 10, "钩子降级注入必须全抓");
    }
    // 10 P95 分位锚点：64 样本 0..63‰，锚点 = (64×95)/100 = 60。
    {
        let mut led = PrecheckLedger::new();
        for i in 0..SAMPLE_CAP as u64 {
            let _ = led.record(1000, 1000 + i);
        }
        let p = led.p95();
        s.add_dyn(
            "P95分位锚点",
            p == Ok(60),
            "64 样本升序 0..63‰ 的 P95 锚点必须落在 60",
        );
    }
    // 11 判定次序：容量先于零基线，零基线先于负开销。
    {
        let mut led = PrecheckLedger::new();
        for _ in 0..SAMPLE_CAP {
            let _ = led.record(BASELINE_TICKS, BASELINE_TICKS + 40);
        }
        let a = led.record(0, 100);
        let mut led2 = PrecheckLedger::new();
        let b = led2.record(0, 0);
        s.add_dyn(
            "判序容量先于零基线",
            a == Err(PrecheckCode::SampleCap) && b == Err(PrecheckCode::ZeroBaseline),
            "容量判据先于基线判据，基线判据先于负开销判据",
        );
    }
    // 12 拒止：被拒样本不入账。
    {
        let mut led = PrecheckLedger::new();
        let before = led.samples_len();
        let _ = led.record(BASELINE_TICKS, BASELINE_TICKS - 5);
        let _ = led.record(0, 100);
        s.add_dyn(
            "拒后账不增",
            led.samples_len() == before,
            "被拒样本不得进入样本账",
        );
    }
    // 13 状态机：单向晋级与倒退拒。
    {
        let mut led = PrecheckLedger::new();
        let r1 = led.promote_to(HookState::Disabled);
        let r2 = led.promote_to(HookState::Enabled);
        let r3 = led.promote_to(HookState::Enforced);
        let r4 = led.promote_to(HookState::Enabled);
        let r5 = led.promote_to(HookState::Disabled);
        s.add_dyn(
            "状态机单向晋级",
            r1.is_ok() && r2.is_ok() && r3.is_ok()
                && r4 == Err(PrecheckCode::StateRegress)
                && r5 == Err(PrecheckCode::StateRegress),
            "只许晋级，倒退显性拒",
        );
    }
    // 14 闸位组合非法：空闸列表拒绝。
    {
        let mut led = PrecheckLedger::new();
        let r = led.hooked_cost(&[]);
        s.add_dyn("空闸列表拒", r == Err(PrecheckCode::HookMismatch), "至少要挂一道闸");
    }
    // 15 对账守恒。
    {
        let mut led = PrecheckLedger::new();
        for _ in 0..10u32 {
            let _ = led.record(BASELINE_TICKS, BASELINE_TICKS - 1);
            led.note_detected();
        }
        s.add_dyn("注入账守恒", led.conserved() && led.injected() == 10, "注入=检出+逃逸，逃逸恒 0");
    }
    // 16 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同种子 LCG 两次建账逐字段相等");
    // 17 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 18 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_flow_p95_within_budget() {
        let mut led = PrecheckLedger::new();
        let _ = led.promote_to(HookState::Enforced);
        let hooks = [HookKind::Boundary, HookKind::WProtect, HookKind::Crc];
        for _ in 0..16u32 {
            let cost = led.hooked_cost(&hooks).map_err(|_| ()).unwrap_or(BASELINE_TICKS + 40);
            assert!(led.record(BASELINE_TICKS, cost).is_ok());
        }
        assert!(led.p95_within_budget());
        assert_eq!(led.breaches(), 0);
    }

    #[test]
    fn budget_boundary_exact_and_plus_one() {
        let mut led = PrecheckLedger::new();
        assert_eq!(led.record(BASELINE_TICKS, BASELINE_TICKS + 50), Ok(50));
        assert_eq!(led.breaches(), 0, "恰在红线不违约");
        assert_eq!(led.record(BASELINE_TICKS, BASELINE_TICKS + 51), Ok(51));
        assert_eq!(led.breaches(), 1, "红线+1 即时记违约");
    }

    #[test]
    fn zero_baseline_and_negative_overhead() {
        let mut led = PrecheckLedger::new();
        assert_eq!(led.record(0, 100), Err(PrecheckCode::ZeroBaseline));
        assert_eq!(led.record(BASELINE_TICKS, BASELINE_TICKS - 1), Err(PrecheckCode::NegativeOverhead));
    }

    #[test]
    fn permille_overflow_rejected() {
        let mut led = PrecheckLedger::new();
        assert_eq!(led.record(1, u64::MAX), Err(PrecheckCode::PermilleOverflow));
    }

    #[test]
    fn sample_cap_exhausted() {
        let mut led = PrecheckLedger::new();
        for _ in 0..SAMPLE_CAP {
            assert!(led.record(BASELINE_TICKS, BASELINE_TICKS + 40).is_ok());
        }
        assert_eq!(led.record(BASELINE_TICKS, BASELINE_TICKS + 40), Err(PrecheckCode::SampleCap));
    }

    #[test]
    fn overspend_injection_all_caught() {
        let mut led = PrecheckLedger::new();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            if let Ok(pm) = led.record(BASELINE_TICKS, BASELINE_TICKS + 200) {
                if pm > BREACH_PERMILLE {
                    caught += 1;
                    led.note_detected();
                }
            }
        }
        assert_eq!(caught, 10, "超标注入必须 10/10 检出");
        assert!(led.conserved());
    }

    #[test]
    fn negative_overhead_injection_all_caught() {
        let mut led = PrecheckLedger::new();
        for _ in 0..10u32 {
            assert_eq!(
                led.record(BASELINE_TICKS, BASELINE_TICKS - 1),
                Err(PrecheckCode::NegativeOverhead),
                "负开销注入必须全抓"
            );
            led.note_detected();
        }
        assert!(led.conserved());
    }

    #[test]
    fn state_regress_injection_all_caught() {
        let mut led = PrecheckLedger::new();
        let _ = led.promote_to(HookState::Enforced);
        for _ in 0..10u32 {
            assert_eq!(led.promote_to(HookState::Disabled), Err(PrecheckCode::StateRegress));
            led.note_detected();
        }
        assert!(led.conserved());
    }

    #[test]
    fn p95_anchor_on_64_samples() {
        let mut led = PrecheckLedger::new();
        for i in 0..SAMPLE_CAP as u64 {
            let _ = led.record(1000, 1000 + i);
        }
        assert_eq!(led.p95(), Ok(60), "(64×95)/100=60 号位即 P95 锚点");
    }

    #[test]
    fn judge_order_cap_then_baseline_then_negative() {
        let mut led = PrecheckLedger::new();
        for _ in 0..SAMPLE_CAP {
            let _ = led.record(BASELINE_TICKS, BASELINE_TICKS + 40);
        }
        assert_eq!(led.record(0, 100), Err(PrecheckCode::SampleCap), "容量判据先于基线判据");
        let mut led2 = PrecheckLedger::new();
        assert_eq!(led2.record(0, 0), Err(PrecheckCode::ZeroBaseline), "零基线判据先于负开销判据");
    }

    #[test]
    fn rejected_samples_never_recorded() {
        let mut led = PrecheckLedger::new();
        let _ = led.record(BASELINE_TICKS, BASELINE_TICKS + 40);
        let before = led.samples_len();
        let _ = led.record(BASELINE_TICKS, BASELINE_TICKS - 5);
        let _ = led.record(0, 100);
        assert_eq!(led.samples_len(), before);
    }

    #[test]
    fn state_machine_forward_only() {
        let mut led = PrecheckLedger::new();
        assert!(led.promote_to(HookState::Disabled).is_ok());
        assert!(led.promote_to(HookState::Enabled).is_ok());
        assert!(led.promote_to(HookState::Enforced).is_ok());
        assert_eq!(led.promote_to(HookState::Enabled), Err(PrecheckCode::StateRegress));
        assert_eq!(led.promote_to(HookState::Disabled), Err(PrecheckCode::StateRegress));
        assert!(led.state_is(HookState::Enforced));
    }

    #[test]
    fn empty_hooks_rejected() {
        let mut led = PrecheckLedger::new();
        assert_eq!(led.hooked_cost(&[]), Err(PrecheckCode::HookMismatch));
    }

    #[test]
    fn dual_run_identical_ledger() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_holds() {
        assert!(seg_guard_ok());
    }

    #[test]
    fn detail_texts_closed_set() {
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
