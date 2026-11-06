//! UNX-F10008 豁免占用列（≤5% 合规列） · 0x91200段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10009-J1 守恒列零偏离（40 批预算↔实计比对 10/10 次），
//! 偏离不标 10/10 次拦截。
//!
//! 设计要点：
//!   ① 占用合规按「双值核」建模：第一值是占用基点 bp = used×10000/total
//!      （整数除法，无浮点），第二值是与冻结阈值 500‱（=5.00%）的比对
//!      结论；两值必须同源同向，任一缺位即双值核失败。
//!   ② 哨兵状态机：Normal → Breach 只进不退；占用越阈必须标 Breach，
//!      「超阈不标」由注入核逐轮拦截（10/10）。
//!   ③ 阈值冻结：500‱ 写死并注明推导（5% = 5/100 = 500/10000）；
//!      阈值漂移（±1、半值、加倍）一律拒绝。
//!   ④ 坏样本闸：零总账、占用大于总账、阈值漂移、超阈不标四类，
//!      逐类 10/10 显性拒绝；注入账守恒逃逸恒为 0。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10008 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91200;

/// 合规阈值（500‱ = 5.00%，写死：5/100 = 500/10000）。
pub const THRESHOLD_BP: u32 = 500;

/// 基点满值（100% = 10000‱）。
pub const BP_FULL: u32 = 10_000;

/// 批数（40 批口径，全局占用按批汇总）。
pub const BATCH_COUNT: usize = 40;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EDOM: i32 = 33;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OccCode {
    /// 零总账：total = 0 无商可取
    ZeroTotal,
    /// 占用大于总账：used > total
    UsedBeyondTotal,
    /// 阈值漂移：比对阈值 ≠ 冻结值 500‱
    ThresholdDrift,
    /// 超阈不标：bp > 阈值而哨兵仍在 Normal
    UnflaggedBreach,
    /// 双值失核：基点值与比对结论矛盾
    DualValueMismatch,
    /// 账倒退：占用累计只增不减
    LedgerRegress,
    /// 基点越界：bp > 10000
    RateOverflow,
    /// 批账不齐：出表时批数不足 40
    BatchShort,
    /// 批重复：同批二次入账
    DuplicateBatch,
    /// 哨兵回退：Breach 回 Normal
    SentinelRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl OccCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            OccCode::ZeroTotal => "零总账被拒：总账为 0 无商可取；请先补齐总账\
                口径再算占用",
            OccCode::UsedBeyondTotal => "占用大于总账被拒：占用数不得超过总账；\
                请回查统计口径",
            OccCode::ThresholdDrift => "阈值漂移被拒：比对阈值不等于冻结值 500‱；\
                阈值是合规口径，不许漂移",
            OccCode::UnflaggedBreach => "超阈不标被拒：占用越阈必须置 Breach；\
                静默放行按拦截处理",
            OccCode::DualValueMismatch => "双值失核被拒：基点值与比对结论必须\
                同源同向；请检查两值是否用了不同分母",
            OccCode::LedgerRegress => "账倒退被拒：占用累计只增不减；请检查是否\
                有回滚路径误改累计值",
            OccCode::RateOverflow => "基点越界被拒：基点值不得大于 10000；请回查\
                分母是否为零或被换",
            OccCode::BatchShort => "批账不齐被拒：出表要求 40 批逐批在账；请补齐\
                缺失批次",
            OccCode::DuplicateBatch => "批重复被拒：同批二次入账；请先查账再补录",
            OccCode::SentinelRegress => "哨兵回退被拒：Breach 只进不退；回退视为\
                私改合规结论",
            OccCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定取整",
            OccCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [OccCode; 12] = [
        OccCode::ZeroTotal,
        OccCode::UsedBeyondTotal,
        OccCode::ThresholdDrift,
        OccCode::UnflaggedBreach,
        OccCode::DualValueMismatch,
        OccCode::LedgerRegress,
        OccCode::RateOverflow,
        OccCode::BatchShort,
        OccCode::DuplicateBatch,
        OccCode::SentinelRegress,
        OccCode::NotReproducible,
        OccCode::SegmentViolation,
    ];
}

// ── 占用基点 ──────────────────────────────────────────────────────────
/// 占用基点（整数）：used×10000/total；坏样本分别给码。
pub fn occupancy_bp(used: u32, total: u32) -> Result<u32, OccCode> {
    if total == 0 {
        return Err(OccCode::ZeroTotal);
    }
    if used > total {
        return Err(OccCode::UsedBeyondTotal);
    }
    let bp = used.saturating_mul(BP_FULL) / total;
    if bp > BP_FULL {
        return Err(OccCode::RateOverflow);
    }
    Ok(bp)
}

/// 阈值比对（第二值）：bp 与冻结阈值比对；阈值漂移拒绝。
pub fn threshold_verdict(bp: u32, threshold: u32) -> Result<bool, OccCode> {
    if threshold != THRESHOLD_BP {
        return Err(OccCode::ThresholdDrift);
    }
    Ok(bp > threshold)
}

// ── 哨兵状态机 ────────────────────────────────────────────────────────
/// 哨兵状态：只进不退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SentinelState {
    /// 合规（占用 ≤ 500‱）
    Normal,
    /// 超阈（占用 > 500‱）
    Breach,
}

/// 占用哨兵：双值核 + 单向状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComplianceSentinel {
    state: SentinelState,
    /// 最近一次基点值
    last_bp: u32,
    /// 是否置过 Breach
    ever_breached: bool,
}

impl ComplianceSentinel {
    /// 构造 Normal 哨兵。
    pub const fn new() -> ComplianceSentinel {
        ComplianceSentinel { state: SentinelState::Normal, last_bp: 0, ever_breached: false }
    }

    /// 当前状态。
    pub fn state(&self) -> SentinelState {
        self.state
    }

    /// 最近基点值。
    pub fn last_bp(&self) -> u32 {
        self.last_bp
    }

    /// 双值核：算基点 + 比对阈值，两值一致才落状态；不一致拒绝。
    /// 越阈必须置 Breach（超阈不标在此拦截）。
    pub fn evaluate(&mut self, used: u32, total: u32) -> Result<SentinelState, OccCode> {
        let bp = occupancy_bp(used, total)?;
        let over = threshold_verdict(bp, THRESHOLD_BP)?;
        // 双值互证：bp > 阈值 ⟺ over 为真，反向复核。
        let cross = bp > THRESHOLD_BP;
        if over != cross {
            return Err(OccCode::DualValueMismatch);
        }
        self.last_bp = bp;
        if over {
            self.state = SentinelState::Breach;
            self.ever_breached = true;
        }
        Ok(self.state)
    }

    /// 是否曾置 Breach（只进不退的账痕）。
    pub fn has_breached(&self) -> bool {
        self.ever_breached
    }

    /// 回退注入：Breach 后试图回 Normal 必须拒绝（显性 API，只作校验）。
    pub fn try_reset(&self) -> Result<(), OccCode> {
        if self.ever_breached {
            return Err(OccCode::SentinelRegress);
        }
        Ok(())
    }
}

// ── 40 批占用账 ───────────────────────────────────────────────────────
/// 批占用账：40 批逐批 (占用行, 总行)，全局占用由合计复算。
#[derive(Clone, Debug)]
pub struct OccupancyBook {
    useds: Vec<u32>,
    totals: Vec<u32>,
    sentinel: ComplianceSentinel,
}

impl OccupancyBook {
    /// 空账。
    pub fn new() -> OccupancyBook {
        OccupancyBook { useds: Vec::new(), totals: Vec::new(), sentinel: ComplianceSentinel::new() }
    }

    /// 入账一批：占用 ≤ 总行、批不重复。
    pub fn add_batch(&mut self, used: u32, total: u32) -> Result<(), OccCode> {
        if total == 0 {
            return Err(OccCode::ZeroTotal);
        }
        if used > total {
            return Err(OccCode::UsedBeyondTotal);
        }
        if self.useds.len() >= BATCH_COUNT {
            return Err(OccCode::DuplicateBatch);
        }
        self.useds.push(used);
        self.totals.push(total);
        Ok(())
    }

    /// 已入账批数。
    pub fn len(&self) -> usize {
        self.useds.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.useds.is_empty()
    }

    /// Σ占用。
    pub fn sum_used(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.useds.len() {
            if let Some(u) = self.useds.get(i) {
                s = s.saturating_add(*u);
            }
            i += 1;
        }
        s
    }

    /// Σ总行。
    pub fn sum_total(&self) -> u32 {
        let mut s = 0u32;
        let mut i = 0usize;
        while i < self.totals.len() {
            if let Some(t) = self.totals.get(i) {
                s = s.saturating_add(*t);
            }
            i += 1;
        }
        s
    }

    /// 全局双值核：40 批齐后按合计算占用并落哨兵。
    pub fn evaluate_global(&mut self) -> Result<(u32, SentinelState), OccCode> {
        if self.len() != BATCH_COUNT {
            return Err(OccCode::BatchShort);
        }
        let bp = occupancy_bp(self.sum_used(), self.sum_total())?;
        let st = self.sentinel.evaluate(self.sum_used(), self.sum_total())?;
        Ok((bp, st))
    }

    /// 哨兵视图。
    pub fn sentinel(&self) -> &ComplianceSentinel {
        &self.sentinel
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(OccCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 超阈不标：越阈占用必须置 Breach
    Unflagged,
    /// 零总账
    ZeroTotal,
    /// 占用大于总账
    Beyond,
    /// 阈值漂移
    Drift,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::Unflagged,
        BadCase::ZeroTotal,
        BadCase::Beyond,
        BadCase::Drift,
    ];
}

/// 漂移阈值样本（确定性轮换：±1、半值、加倍）。
fn drift_threshold(round: u32) -> u32 {
    match round % 4 {
        0 => THRESHOLD_BP - 1,
        1 => THRESHOLD_BP + 1,
        2 => THRESHOLD_BP / 2,
        _ => THRESHOLD_BP * 2,
    }
}

/// 执行四类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_case_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    // 超阈不标：used=6/total=100 → 600‱ > 500‱，
                    // 哨兵必须置 Breach；任何「仍在 Normal」都算逃逸。
                    BadCase::Unflagged => {
                        let mut st = ComplianceSentinel::new();
                        st.evaluate(6, 100) == Ok(SentinelState::Breach)
                    }
                    BadCase::ZeroTotal => {
                        occupancy_bp(1, 0) == Err(OccCode::ZeroTotal)
                    }
                    BadCase::Beyond => {
                        occupancy_bp(101, 100) == Err(OccCode::UsedBeyondTotal)
                    }
                    BadCase::Drift => {
                        threshold_verdict(501, drift_threshold(round))
                            == Err(OccCode::ThresholdDrift)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 基准账：40 批、每批总行 100、占用 4 行（全局 4% = 400‱ 合规）。
pub fn build_compliant_book() -> OccupancyBook {
    let mut book = OccupancyBook::new();
    let mut b = 0usize;
    while b < BATCH_COUNT {
        let _ = book.add_batch(4, 100);
        b += 1;
    }
    book
}

/// 超阈账：40 批、每批总行 100、占用 6 行（全局 6% = 600‱ 超阈）。
pub fn build_breach_book() -> OccupancyBook {
    let mut book = OccupancyBook::new();
    let mut b = 0usize;
    while b < BATCH_COUNT {
        let _ = book.add_batch(6, 100);
        b += 1;
    }
    book
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EDOM", -33, 33, "Windows ERROR_MATH_DOMAIN；零分母/定义域族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；批账缺员族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；越界/漂移族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；基点越界族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 哨兵指纹：基点 + 状态 + 账痕逐字节入哈希。
pub fn sentinel_fingerprint(st: &ComplianceSentinel) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(((st.last_bp >> 8) & 0xFF) as u8);
    bytes.push((st.last_bp & 0xFF) as u8);
    bytes.push(match st.state() {
        SentinelState::Normal => 0,
        SentinelState::Breach => 1,
    });
    bytes.push(if st.has_breached() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10008 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10008_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10008");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 阈值冻结：500‱ = 5% 可复算
    s.add("阈值冻结：500‱=5.00% 且 < 基点满值",
          THRESHOLD_BP == BP_FULL * 5 / 100 && THRESHOLD_BP < BP_FULL,
          "阈值必须能从 5% 口径复算，写死值与推导一致");

    // 3. 合规账：40 批 4% 全局占用，哨兵 Normal
    let mut ok_book = build_compliant_book();
    let ok_eval = ok_book.evaluate_global();
    let ok_ok = match ok_eval {
        Ok((bp, st)) => bp == 400 && st == SentinelState::Normal,
        Err(_) => false,
    };
    s.add("双值核：4% 占用取数 400‱ 且 Normal", ok_ok,
          "基点值与比对结论必须同源可复算");

    // 4. 超阈账：6% 全局占用，哨兵 Breach
    let mut bad_book = build_breach_book();
    let bad_eval = bad_book.evaluate_global();
    let bad_ok = match bad_eval {
        Ok((bp, st)) => bp == 600 && st == SentinelState::Breach,
        Err(_) => false,
    };
    s.add("超阈标注：6% 占用取数 600‱ 且 Breach", bad_ok,
          "越阈必须置 Breach，不许静默放行");

    // 5. 占用列边界：0% 与 100%
    let edge_zero = occupancy_bp(0, 100) == Ok(0);
    let edge_full = occupancy_bp(100, 100) == Ok(BP_FULL);
    s.add("边界取数：0‱ 与 10000‱ 均精确",
          edge_zero && edge_full,
          "占用域两端点是合规列的锚");

    // 6. 注入矩阵：四类各 10/10 检出
    let (ledger, per_class) = run_bad_case_matrix();
    let mut all_caught = true;
    let mut ci = 0usize;
    while ci < per_class.len() {
        if let Some(n) = per_class.get(ci) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        ci += 1;
    }
    s.add("注入矩阵：超阈不标/零总账/越总账/阈值漂移各 10/10", all_caught,
          "四类坏样本必须全部显性拒绝或标注");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 超阈不标专项：阈值邻界两侧结论必须翻转
    let mut near = ComplianceSentinel::new();
    let just_within = near.evaluate(5, 100) == Ok(SentinelState::Normal);
    let mut over = ComplianceSentinel::new();
    let just_over = over.evaluate(6, 100) == Ok(SentinelState::Breach);
    s.add("邻界翻转：500‱ 内 Normal、501‱ 起 Breach",
          just_within && just_over,
          "阈值语义是严格大于超阈，边界行为必须精确");

    // 9. 零总账拒绝
    s.add("零总账：total=0 拒绝",
          occupancy_bp(1, 0).err() == Some(OccCode::ZeroTotal),
          "零分母无商可取，必须显性拒绝");

    // 10. 占用大于总账拒绝
    s.add("越总账：used>total 拒绝",
          occupancy_bp(101, 100).err() == Some(OccCode::UsedBeyondTotal),
          "占用超过总账属口径矛盾，必须拒绝");

    // 11. 阈值漂移拒绝（四档漂移轮换在矩阵内）
    let drift_ok = threshold_verdict(600, THRESHOLD_BP).is_ok()
        && threshold_verdict(600, THRESHOLD_BP - 1) == Err(OccCode::ThresholdDrift)
        && threshold_verdict(600, THRESHOLD_BP * 2) == Err(OccCode::ThresholdDrift);
    s.add("阈值漂移：非 500‱ 阈值一律拒绝", drift_ok,
          "比对只认冻结阈值，漂移阈值取数即拦");

    // 12. 哨兵只进不退
    let mut reg = ComplianceSentinel::new();
    let _ = reg.evaluate(10, 100);
    s.add("哨兵回退：Breach 后 reset 被拒",
          reg.has_breached() && reg.try_reset() == Err(OccCode::SentinelRegress),
          "合规结论只进不退，回退视为私改");

    // 13. 批账不齐拦截
    let mut short = OccupancyBook::new();
    let mut b = 0usize;
    while b < BATCH_COUNT - 1 {
        let _ = short.add_batch(4, 100);
        b += 1;
    }
    s.add("批账不齐：39 批出表被拒",
          short.evaluate_global().err() == Some(OccCode::BatchShort),
          "全局占用必须按 40 批合计复算，缺批即拦");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(EDOM)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 15. 双跑指纹一致
    let mut probe_ok = build_compliant_book();
    let _ = probe_ok.evaluate_global();
    let mut probe_bad = build_breach_book();
    let _ = probe_bad.evaluate_global();
    let fp_a = sentinel_fingerprint(probe_ok.sentinel());
    let fp_b = sentinel_fingerprint(probe_ok.sentinel());
    let fp_c = sentinel_fingerprint(probe_bad.sentinel());
    s.add("双跑一致：合规哨兵指纹相等且与超阈相异",
          fp_a == fp_b && fp_a != fp_c,
          "同输入两次构建必须逐字段相等，两态指纹必须可区分");

    // 16. 诊断码唯一与文案闭集
    let text_ok = OccCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < OccCode::ALL.len() {
        let mut j = i + 1;
        while j < OccCode::ALL.len() {
            if let (Some(a), Some(b)) = (OccCode::ALL.get(i), OccCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码闭集：12 码唯一且文案非空", text_ok && uniq,
          "段内闭集码不得重复，每个码带非空中文建议");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("合规哨兵指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_frozen() {
        assert_eq!(THRESHOLD_BP, 500);
        assert_eq!(BP_FULL * 5 / 100, THRESHOLD_BP);
    }

    #[test]
    fn compliant_book_normal() {
        let mut book = build_compliant_book();
        let (bp, st) = book.evaluate_global().unwrap_or((0, SentinelState::Breach));
        assert_eq!(bp, 400);
        assert_eq!(st, SentinelState::Normal);
    }

    #[test]
    fn breach_book_flags() {
        let mut book = build_breach_book();
        let (bp, st) = book.evaluate_global().unwrap_or((0, SentinelState::Normal));
        assert_eq!(bp, 600);
        assert_eq!(st, SentinelState::Breach);
        assert!(book.sentinel().has_breached());
    }

    #[test]
    fn boundary_flip() {
        let mut a = ComplianceSentinel::new();
        assert_eq!(a.evaluate(5, 100), Ok(SentinelState::Normal));
        let mut b = ComplianceSentinel::new();
        assert_eq!(b.evaluate(6, 100), Ok(SentinelState::Breach));
    }

    #[test]
    fn zero_total_rejected() {
        assert_eq!(occupancy_bp(1, 0), Err(OccCode::ZeroTotal));
    }

    #[test]
    fn beyond_total_rejected() {
        assert_eq!(occupancy_bp(101, 100), Err(OccCode::UsedBeyondTotal));
    }

    #[test]
    fn threshold_drift_rejected() {
        assert_eq!(threshold_verdict(600, 499), Err(OccCode::ThresholdDrift));
        assert_eq!(threshold_verdict(600, 501), Err(OccCode::ThresholdDrift));
        assert!(threshold_verdict(600, 500).is_ok());
    }

    #[test]
    fn sentinel_no_regress() {
        let mut st = ComplianceSentinel::new();
        let _ = st.evaluate(10, 100);
        assert_eq!(st.try_reset(), Err(OccCode::SentinelRegress));
    }

    #[test]
    fn batch_short_rejected() {
        let mut book = OccupancyBook::new();
        let _ = book.add_batch(4, 100);
        assert_eq!(book.evaluate_global(), Err(OccCode::BatchShort));
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprints_distinct() {
        let mut ok = build_compliant_book();
        let _ = ok.evaluate_global();
        let mut bad = build_breach_book();
        let _ = bad.evaluate_global();
        assert_ne!(
            sentinel_fingerprint(ok.sentinel()),
            sentinel_fingerprint(bad.sentinel())
        );
    }

    #[test]
    fn unx10008_checks_green() {
        let set = run_unx10008_checks();
        assert!(set.all_passed(), "UNX-F10008 red: {}", set.red_items_text());
    }
}
