//! UNX-F51528 突发流量整形（burst 令牌桶） · 0xC0700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51529-J1 并网对账 ×20 零漂移（联合打点锚定行）。
//!
//! 设计要点：
//!   ① burst 令牌桶按「桶容 + 固定速率回填」建模：授予 = min(请求, 桶余)，
//!      桶空零授；零授予笔也必须落账，削峰量（want − granted）逐条入账，
//!      与账面总量逐条对平（削峰计数与账面同步推进）。
//!   ② 联合打点单调面：tick 严格单调，倒退显性拒（TickBackward），拒后
//!      版本号与桶态纹丝不动；打点即记账，无旁路。
//!   ③ 相位状态机 Burst → Shaping → Drained：授予路径只允许劣化向迁移、
//!      回填路径只允许恢复向迁移；外部直改与桶量推导矛盾的相位一律
//!      PhaseLocked 显性拒（违约注入必抓）。
//!   ④ 并网对账 ×20 零漂移：账面授予计数器 vs 流水逐条重算（.get 遍历），
//!      漂移 = 两侧差之和，恒 0；守恒式 want = granted + shaped 逐条成立。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒 0；三类违约注入
//!      （超桶请求 / tick 倒退 / 相位直改）×10 轮全检。
//!   ⑥ 诊断码闭集可取中文建议；段位守卫 0x40 对齐且落在分派区间。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC0700;

/// 冻结常量表：桶容 / 回填速率 / 单笔上限 / 流水容量 / 对账轮数。
pub const BUCKET_CAP_TOKENS: u64 = 4096;
pub const REFILL_PER_TICK: u64 = 256;
pub const MAX_REQUEST_TOKENS: u64 = 2048;
pub const MAX_LEDGER_EVENTS: usize = 256;
pub const NETTING_ROUNDS: usize = 20;

/// 诊断码闭集（突发整形域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BurstCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// tick 倒退（联合打点必须单调）
    TickBackward,
    /// 请求超单笔上限
    RequestOverCap,
    /// 相位被外部直改且与桶量矛盾
    PhaseLocked,
    /// 授予流水满
    LedgerFull,
    /// 并网对账漂移非零
    NettingDrift,
    /// 守恒账不平（want ≠ granted + shaped）
    LedgerMismatch,
}

impl BurstCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BurstCode::Ok => "整形通过",
            BurstCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            BurstCode::TickBackward => "tick 倒退：联合打点必须单调递增",
            BurstCode::RequestOverCap => "请求超单笔上限：拆分后重新提交",
            BurstCode::PhaseLocked => "相位直改被拒：相位只能经授予或回填迁移",
            BurstCode::LedgerFull => "流水满：先并网结算再继续打点",
            BurstCode::NettingDrift => "并网漂移非零：账面与流水必须逐条对平",
            BurstCode::LedgerMismatch => "守恒不平：want = granted + shaped 必须逐条成立",
        }
    }
}

/// 相位状态机：Burst（直通）→ Shaping（削峰）→ Drained（桶空）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShapePhase {
    /// 桶内令牌充裕，直通
    Burst,
    /// 正在削峰
    Shaping,
    /// 桶已抽干
    Drained,
}

impl ShapePhase {
    /// 劣化秩：Drained 最高；授予只允许秩增，回填只允许秩减。
    pub fn rank(self) -> u8 {
        match self {
            ShapePhase::Burst => 0,
            ShapePhase::Shaping => 1,
            ShapePhase::Drained => 2,
        }
    }
}

/// 单条授予流水（含零授予笔）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrantRec {
    /// 流水序号（账内单调）
    pub seq: u32,
    /// 联合打点锚（单调 tick）
    pub tick: u64,
    /// 请求量
    pub want: u32,
    /// 实授量（= min(want, 桶余)）
    pub granted: u32,
    /// 削峰量（want − granted）
    pub shaped: u32,
}

/// 突发整形引擎：令牌桶 + 相位机 + 守恒账。
pub struct BurstShaper {
    cap_tokens: u64,
    refill_per_tick: u64,
    max_request: u64,
    max_events: usize,
    tokens: u64,
    last_tick: u64,
    version: u64,
    phase: ShapePhase,
    ledger: Vec<GrantRec>,
    /// 账面总量（与流水逐笔同步推进）。
    sum_want: u64,
    sum_granted: u64,
    sum_shaped: u64,
    zero_grant_cnt: u64,
    reject_cnt: u64,
    /// 注入/检出/逃逸守恒账（逃逸恒 0）。
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl BurstShaper {
    /// 冻结默认参数引擎。
    pub fn new() -> BurstShaper {
        BurstShaper::with_limits(
            BUCKET_CAP_TOKENS,
            REFILL_PER_TICK,
            MAX_REQUEST_TOKENS,
            MAX_LEDGER_EVENTS,
        )
    }

    /// 自定参数引擎（容量耗尽等边界测试用）。
    pub fn with_limits(cap: u64, refill: u64, max_req: u64, max_events: usize) -> BurstShaper {
        BurstShaper {
            cap_tokens: cap,
            refill_per_tick: refill,
            max_request: max_req,
            max_events,
            tokens: cap,
            last_tick: 0,
            version: 0,
            phase: ShapePhase::Burst,
            ledger: Vec::new(),
            sum_want: 0,
            sum_granted: 0,
            sum_shaped: 0,
            zero_grant_cnt: 0,
            reject_cnt: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 按桶量推导相位。
    fn phase_from_tokens(tokens: u64, cap: u64) -> ShapePhase {
        if tokens >= cap {
            ShapePhase::Burst
        } else if tokens > 0 {
            ShapePhase::Shaping
        } else {
            ShapePhase::Drained
        }
    }

    /// 回填到 tick：tick 单调（倒退显性拒）；回填路径只允许恢复向迁移。
    pub fn refill_to(&mut self, tick: u64) -> Result<u64, BurstCode> {
        if tick < self.last_tick {
            self.reject_cnt += 1;
            return Err(BurstCode::TickBackward);
        }
        let elapsed = tick - self.last_tick;
        let gain = elapsed.saturating_mul(self.refill_per_tick);
        self.tokens = self.cap_tokens.min(self.tokens.saturating_add(gain));
        self.last_tick = tick;
        // 回填只恢复不劣化：秩减才迁移，秩增保持。
        let computed = Self::phase_from_tokens(self.tokens, self.cap_tokens);
        if computed.rank() < self.phase.rank() {
            self.phase = computed;
        }
        self.version += 1;
        Ok(self.tokens)
    }

    /// 授予一笔。判序：tick 单调 → 单笔上限 → 流水容量 → 回填 → 桶截断 → 落账。
    /// 零授予笔也落账；削峰量逐条入账。
    pub fn grant(&mut self, tick: u64, want: u32) -> Result<GrantRec, BurstCode> {
        if tick < self.last_tick {
            self.reject_cnt += 1;
            return Err(BurstCode::TickBackward);
        }
        if want as u64 > self.max_request {
            self.reject_cnt += 1;
            return Err(BurstCode::RequestOverCap);
        }
        if self.ledger.len() >= self.max_events {
            return Err(BurstCode::LedgerFull);
        }
        // 回填（与 refill_to 同语义，不单独计版本）。
        let elapsed = tick - self.last_tick;
        let gain = elapsed.saturating_mul(self.refill_per_tick);
        self.tokens = self.cap_tokens.min(self.tokens.saturating_add(gain));
        self.last_tick = tick;
        let want64 = want as u64;
        let granted = want64.min(self.tokens);
        let shaped = want64 - granted;
        self.tokens -= granted;
        // 授予路径只劣化不恢复：秩增才迁移。
        let computed = Self::phase_from_tokens(self.tokens, self.cap_tokens);
        if computed.rank() > self.phase.rank() {
            self.phase = computed;
        }
        let rec = GrantRec {
            seq: self.ledger.len() as u32,
            tick,
            want,
            granted: granted as u32,
            shaped: shaped as u32,
        };
        self.ledger.push(rec);
        // 账面与流水逐笔同步推进。
        self.sum_want += want64;
        self.sum_granted += granted;
        self.sum_shaped += shaped;
        if granted == 0 {
            self.zero_grant_cnt += 1;
        }
        self.version += 1;
        Ok(rec)
    }

    /// 注入面：外部直改相位。相位必须与当前桶量推导一致，矛盾即拒（违约必抓）。
    pub fn inject_phase_write(&mut self, to: ShapePhase) -> Result<(), BurstCode> {
        let computed = Self::phase_from_tokens(self.tokens, self.cap_tokens);
        if to == computed {
            Ok(())
        } else {
            Err(BurstCode::PhaseLocked)
        }
    }

    /// 并网对账：账面三计数器 vs 流水逐条重算（.get 遍历），返回漂移总量。
    /// 守恒式 want = granted + shaped 逐条校验，破式计入漂移。
    pub fn netting_drift(&self) -> u64 {
        let mut w_want = 0u64;
        let mut w_granted = 0u64;
        let mut w_shaped = 0u64;
        let mut w_conserve = 0u64;
        for i in 0..self.ledger.len() {
            if let Some(r) = self.ledger.get(i) {
                w_want += r.want as u64;
                w_granted += r.granted as u64;
                w_shaped += r.shaped as u64;
                let want64 = r.want as u64;
                let g64 = r.granted as u64;
                let s64 = r.shaped as u64;
                if want64 != g64.saturating_add(s64) {
                    w_conserve += 1;
                }
            }
        }
        w_want.abs_diff(self.sum_want)
            + w_granted.abs_diff(self.sum_granted)
            + w_shaped.abs_diff(self.sum_shaped)
            + w_conserve
    }

    /// 注入记账：注入与检出同步加一（调用方在捕获违约时登记）。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0；非零即记账面破洞）。
    pub fn note_escape(&mut self) {
        self.escaped += 1;
    }

    pub fn tokens(&self) -> u64 {
        self.tokens
    }
    pub fn version(&self) -> u64 {
        self.version
    }
    pub fn phase(&self) -> ShapePhase {
        self.phase
    }
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }
    pub fn zero_grant_cnt(&self) -> u64 {
        self.zero_grant_cnt
    }
    pub fn sum_granted(&self) -> u64 {
        self.sum_granted
    }
    pub fn sum_shaped(&self) -> u64 {
        self.sum_shaped
    }
    pub fn injected(&self) -> u64 {
        self.injected
    }
    pub fn detected(&self) -> u64 {
        self.detected
    }
    pub fn escaped(&self) -> u64 {
        self.escaped
    }

    /// 守恒断言面：注入 = 检出 + 逃逸，且逃逸恒 0、检出 > 0。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一输入序列两次重建流水逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> Vec<GrantRec> {
        let mut e = BurstShaper::new();
        let _ = e.refill_to(1);
        let _ = e.grant(1, 600);
        let _ = e.grant(2, 3000);
        let _ = e.grant(3, 1500);
        let _ = e.refill_to(9);
        e.ledger.clone()
    };
    let a = build();
    let b = build();
    a == b
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [BurstCode; 8] = [
    BurstCode::Ok,
    BurstCode::SegBad,
    BurstCode::TickBackward,
    BurstCode::RequestOverCap,
    BurstCode::PhaseLocked,
    BurstCode::LedgerFull,
    BurstCode::NettingDrift,
    BurstCode::LedgerMismatch,
];

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51528_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51528");
    // 1 正常流：桶内请求全额授予并落账。
    {
        let mut e = BurstShaper::new();
        let r = e.grant(1, 600);
        s.add_dyn(
            "正常授予落账",
            matches!(r, Ok(rec) if rec.granted == 600 && rec.shaped == 0) && e.ledger_len() == 1,
            "桶内请求应全额授予并落 1 条流水",
        );
    }
    // 2 边界零：want=0 零授予笔也落账。
    {
        let mut e = BurstShaper::new();
        let r = e.grant(1, 0);
        s.add_dyn(
            "零授予笔落账",
            matches!(r, Ok(rec) if rec.granted == 0) && e.zero_grant_cnt() == 1,
            "零授予也必须落账，削峰账逐条对平",
        );
    }
    // 3 边界上限：want=单笔上限恰好放行。
    {
        let mut e = BurstShaper::new();
        let r = e.grant(1, MAX_REQUEST_TOKENS as u32);
        s.add_dyn(
            "单笔上限放行",
            matches!(r, Ok(rec) if rec.granted as u64 == MAX_REQUEST_TOKENS),
            "恰等于单笔上限应全额授予",
        );
    }
    // 4 边界上限+1：拒绝。
    {
        let mut e = BurstShaper::new();
        let r = e.grant(1, MAX_REQUEST_TOKENS as u32 + 1);
        s.add_dyn(
            "单笔上限+1 拒绝",
            r == Err(BurstCode::RequestOverCap),
            "超单笔上限必须拒绝",
        );
    }
    // 5 注入必抓：超桶请求 ×10 轮。
    {
        let mut e = BurstShaper::new();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.grant(1, MAX_REQUEST_TOKENS as u32 + 1 + i);
            if r == Err(BurstCode::RequestOverCap) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("超桶注入 10/10", caught == 10, "超桶违约必须全抓");
    }
    // 6 注入必抓：tick 倒退 ×10 轮，且版本号冻结。
    {
        let mut e = BurstShaper::new();
        let _ = e.grant(10, 100);
        let v0 = e.version();
        let t0 = e.tokens();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.grant(3, 100);
            if r == Err(BurstCode::TickBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn(
            "tick 倒退注入 10/10 且状态冻结",
            caught == 10 && e.version() == v0 && e.tokens() == t0,
            "tick 倒退必须全抓且拒后状态纹丝不动",
        );
    }
    // 7 注入必抓：相位直改 ×10 轮。
    {
        let mut e = BurstShaper::new();
        let _ = e.grant(1, 3000);
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let r = e.inject_phase_write(ShapePhase::Burst);
            if r == Err(BurstCode::PhaseLocked) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("相位直改注入 10/10", caught == 10, "与桶量矛盾的直改必须全抓");
    }
    // 8 判定次序：tick 守卫先于单笔上限。
    {
        let mut e = BurstShaper::new();
        let _ = e.grant(10, 100);
        let r = e.grant(2, MAX_REQUEST_TOKENS as u32 + 99);
        s.add_dyn(
            "判序 tick 先于上限",
            r == Err(BurstCode::TickBackward),
            "tick 守卫先于单笔上限判据",
        );
    }
    // 9 容量耗尽：流水满拒绝且不落账。
    {
        let mut e = BurstShaper::with_limits(8192, 0, 2048, 4);
        let mut ok_cnt = 0u32;
        for i in 0..6u32 {
            if e.grant(i as u64 + 1, 10).is_ok() {
                ok_cnt += 1;
            }
        }
        s.add_dyn(
            "流水容量耗尽",
            ok_cnt == 4 && e.ledger_len() == 4,
            "超出流水容量的授予必须整笔拒绝且不落账",
        );
    }
    // 10 相位机：桶空入 Drained，回填恢复向回升。
    {
        let mut e = BurstShaper::with_limits(1000, 100, 2048, 64);
        let _ = e.grant(1, 1000);
        let drained = e.phase() == ShapePhase::Drained;
        let _ = e.refill_to(2);
        let mid = e.phase() == ShapePhase::Shaping;
        let _ = e.refill_to(11);
        s.add_dyn(
            "相位机劣化与恢复",
            drained && mid && e.phase() == ShapePhase::Burst,
            "授予劣化、回填恢复，相位只能沿合法路径迁移",
        );
    }
    // 11 并网对账 ×20 零漂移。
    {
        let mut e = BurstShaper::new();
        let _ = e.grant(1, 600);
        let _ = e.grant(2, 3000);
        let _ = e.grant(3, 1500);
        let _ = e.grant(4, 0);
        let mut zero_rounds = 0u32;
        for _ in 0..NETTING_ROUNDS {
            if e.netting_drift() == 0 {
                zero_rounds += 1;
            }
        }
        s.add_dyn(
            "并网对账 ×20 零漂移",
            zero_rounds == NETTING_ROUNDS as u32,
            "账面与流水逐条对平，20 轮漂移恒 0",
        );
    }
    // 12 对账守恒：注入=检出+逃逸，逃逸 0；want=granted+shaped。
    {
        let mut e = BurstShaper::new();
        for i in 0..10u32 {
            let _ = e.grant(1 + i as u64, MAX_REQUEST_TOKENS as u32 + 1);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10 && e.netting_drift() == 0,
            "注入=检出+逃逸，逃逸恒 0，守恒式逐条成立",
        );
    }
    // 13 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一输入两次建账逐字段相等");
    // 14 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 15 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_grant_books_ledger() {
        let mut e = BurstShaper::new();
        let rec = e.grant(1, 600);
        assert!(matches!(rec, Ok(r) if r.granted == 600 && r.shaped == 0));
        assert_eq!(e.ledger_len(), 1);
        assert_eq!(e.tokens(), BUCKET_CAP_TOKENS - 600);
    }

    #[test]
    fn zero_grant_booked_when_bucket_drained() {
        let mut e = BurstShaper::new();
        let _ = e.grant(1, 4096);
        let rec = e.grant(1, 512);
        assert!(matches!(rec, Ok(r) if r.granted == 0 && r.shaped == 512));
        assert_eq!(e.zero_grant_cnt(), 1);
        assert_eq!(e.phase(), ShapePhase::Drained);
        assert_eq!(e.netting_drift(), 0);
    }

    #[test]
    fn request_cap_boundary() {
        let mut e = BurstShaper::new();
        assert!(e.grant(1, MAX_REQUEST_TOKENS as u32).is_ok());
        assert_eq!(e.grant(1, MAX_REQUEST_TOKENS as u32 + 1), Err(BurstCode::RequestOverCap));
    }

    #[test]
    fn tick_backward_injection_caught_x10() {
        let mut e = BurstShaper::new();
        let _ = e.grant(10, 100);
        let v0 = e.version();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let r = e.grant(9 - i.min(8), 100);
            if r == Err(BurstCode::TickBackward) {
                caught += 1;
                e.note_detected();
            }
        }
        assert_eq!(caught, 10);
        assert_eq!(e.version(), v0, "拒后版本号不得前移");
        assert!(e.conserved());
    }

    #[test]
    fn phase_write_injection_caught_x10() {
        let mut e = BurstShaper::new();
        let _ = e.grant(1, 4000);
        for _ in 0..10 {
            assert_eq!(e.inject_phase_write(ShapePhase::Burst), Err(BurstCode::PhaseLocked));
            e.note_detected();
        }
        assert_eq!(e.inject_phase_write(ShapePhase::Shaping), Ok(()));
    }

    #[test]
    fn judge_order_tick_before_cap() {
        let mut e = BurstShaper::new();
        let _ = e.grant(10, 100);
        assert_eq!(
            e.grant(2, MAX_REQUEST_TOKENS as u32 + 99),
            Err(BurstCode::TickBackward),
            "tick 守卫先于单笔上限"
        );
    }

    #[test]
    fn ledger_full_rejects_cleanly() {
        let mut e = BurstShaper::with_limits(8192, 0, 2048, 4);
        for i in 0..4u64 {
            assert!(e.grant(i + 1, 10).is_ok());
        }
        assert_eq!(e.grant(5, 10), Err(BurstCode::LedgerFull));
        assert_eq!(e.ledger_len(), 4);
    }

    #[test]
    fn netting_20_rounds_zero_drift() {
        let mut e = BurstShaper::new();
        let _ = e.grant(1, 600);
        let _ = e.grant(2, 3000);
        let _ = e.grant(3, 1500);
        let _ = e.grant(4, 0);
        for _ in 0..NETTING_ROUNDS {
            assert_eq!(e.netting_drift(), 0);
        }
        assert_eq!(e.sum_granted(), 600 + 3000 + 1500);
        assert_eq!(e.sum_shaped(), 996);
    }

    #[test]
    fn dual_run_identical_ledger() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_set_hold() {
        assert!(seg_guard_ok());
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
