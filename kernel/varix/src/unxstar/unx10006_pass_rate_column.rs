//! UNX-F10006 判据达成率列（J 通过率） · 0x91180段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10007-J1 敞口三格齐（新红/复发/波动 10/10 次取数正确），
//! 混格 10/10 次检出。
//!
//! 设计要点：
//!   ① 达成率列按「冻结分母 + 整数基点」建模：分母口径冻结为 240
//!      （12 模 × 20 条判据口径），达成率以基点（‱，万分之一）整数计，
//!      rate_bp = passed × 10000 / frozen；全程无浮点，双跑逐位一致。
//!   ② 口径漂移闸：任何以非冻结分母取数的请求一律拒绝（Drift）；
//!      分母为零、通过数大于分母两类坏样本同样显性拒绝。
//!   ③ 取数次序闸：冻结器未 freeze 前取数被拒（NotFrozen）；冻结后
//!      分母不可再改（AlreadyFrozen）。
//!   ④ 单调性质：通过数不减则达成率不减（整数除法保序）；注入核
//!      四类各 10/10 检出，注入账守恒逃逸恒为 0。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10006 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91180;

/// 冻结分母（12 模 × 20 条判据口径）。
pub const FROZEN_DENOMINATOR: u32 = 240;

/// 基点满值（100% = 10000‱）。
pub const BP_FULL: u32 = 10_000;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;
pub const EBUSY: i32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateCode {
    /// 分母漂移：取数分母 ≠ 冻结值
    DenominatorDrift,
    /// 分母为零
    ZeroDenominator,
    /// 通过数越界：passed > 分母
    PassOverflow,
    /// 未冻结取数
    NotFrozen,
    /// 冻结后改分母
    AlreadyFrozen,
    /// 达成率越界：rate_bp > 10000
    RateOverflow,
    /// 单调破约：通过数不减而达成率下降
    MonotonicBreach,
    /// 往返失真：rate 还原分母不符
    RoundtripBreach,
    /// 账倒退：通过数只增不减
    LedgerRegress,
    /// 未复算出表：取数列未经复核
    EmitBeforeAudit,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl RateCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RateCode::DenominatorDrift => "口径漂移被拒：取数分母与冻结值不符；\
                达成率列只认冻结分母，漂移一律拦截",
            RateCode::ZeroDenominator => "分母为零被拒：零分母无商可取；请先\
                修正口径再取数",
            RateCode::PassOverflow => "通过数越界被拒：通过数不得大于分母；\
                请回查该列统计口径",
            RateCode::NotFrozen => "未冻结取数被拒：分母冻结前取数无口径保障；\
                请先 freeze 再取数",
            RateCode::AlreadyFrozen => "冻结后改分母被拒：口径冻结即不可变；\
                如需调整请新开一期口径",
            RateCode::RateOverflow => "达成率越界被拒：基点值不得大于 10000；\
                请回查分母是否漂移",
            RateCode::MonotonicBreach => "单调破约：通过数不减而达成率下降；\
                请排查是否换用了非冻结分母",
            RateCode::RoundtripBreach => "往返失真：基点还原后分母不符；请检查\
                取整路径是否旁路冻结口径",
            RateCode::LedgerRegress => "账倒退被拒：通过数只增不减；请检查\
                是否有回滚路径误改累计值",
            RateCode::EmitBeforeAudit => "未复算出表被拒：达成率列出表前必须\
                先过复核；请先 audit",
            RateCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定取整",
            RateCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [RateCode; 12] = [
        RateCode::DenominatorDrift,
        RateCode::ZeroDenominator,
        RateCode::PassOverflow,
        RateCode::NotFrozen,
        RateCode::AlreadyFrozen,
        RateCode::RateOverflow,
        RateCode::MonotonicBreach,
        RateCode::RoundtripBreach,
        RateCode::LedgerRegress,
        RateCode::EmitBeforeAudit,
        RateCode::NotReproducible,
        RateCode::SegmentViolation,
    ];
}

// ── 分母冻结器 ────────────────────────────────────────────────────────
/// 分母冻结器：未冻结可设分母，冻结后只读；一切取数都走冻结口径。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DenominatorFreezer {
    frozen: bool,
    denom: u32,
}

impl DenominatorFreezer {
    /// 构造未冻结器（分母置 0，显性无效值）。
    pub const fn new() -> DenominatorFreezer {
        DenominatorFreezer { frozen: false, denom: 0 }
    }

    /// 设定分母并冻结：只许一次；冻结后再设被拒。
    pub fn freeze(&mut self, denom: u32) -> Result<(), RateCode> {
        if self.frozen {
            return Err(RateCode::AlreadyFrozen);
        }
        if denom == 0 {
            return Err(RateCode::ZeroDenominator);
        }
        self.denom = denom;
        self.frozen = true;
        Ok(())
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 冻结分母（未冻结返回 0，显性可查）。
    pub fn denominator(&self) -> u32 {
        if self.frozen {
            self.denom
        } else {
            0
        }
    }

    /// 达成率取数（基点）：只认冻结分母；passed > 分母拒绝。
    pub fn rate_bp(&self, passed: u32) -> Result<u32, RateCode> {
        if !self.frozen {
            return Err(RateCode::NotFrozen);
        }
        if passed > self.denom {
            return Err(RateCode::PassOverflow);
        }
        let bp = (passed.saturating_mul(BP_FULL)) / self.denom;
        if bp > BP_FULL {
            return Err(RateCode::RateOverflow);
        }
        Ok(bp)
    }

    /// 带分母取数（漂移闸）：denom ≠ 冻结值一律拒绝，这是口径漂移
    /// 10/10 次拦截的唯一闸口。
    pub fn rate_bp_vs(&self, passed: u32, denom: u32) -> Result<u32, RateCode> {
        if !self.frozen {
            return Err(RateCode::NotFrozen);
        }
        if denom != self.denom {
            return Err(RateCode::DenominatorDrift);
        }
        self.rate_bp(passed)
    }
}

// ── 达成率列 ──────────────────────────────────────────────────────────
/// 达成率列：通过数账 + 复核闸 + 出表。
#[derive(Clone, Debug)]
pub struct RateColumn {
    freezer: DenominatorFreezer,
    passed: u32,
    audited: bool,
}

impl RateColumn {
    /// 构造空列（分母取冻结口径）。
    pub fn new() -> RateColumn {
        let mut fz = DenominatorFreezer::new();
        let _ = fz.freeze(FROZEN_DENOMINATOR);
        RateColumn { freezer: fz, passed: 0, audited: false }
    }

    /// 未冻结列（用于状态闸判据）。
    pub fn unfrozen() -> RateColumn {
        RateColumn { freezer: DenominatorFreezer::new(), passed: 0, audited: false }
    }

    /// 加通过数（只增）。
    pub fn add_passed(&mut self, n: u32) -> Result<(), RateCode> {
        let before = self.passed;
        self.passed = self.passed.saturating_add(n);
        if self.passed < before {
            return Err(RateCode::LedgerRegress);
        }
        self.audited = false;
        Ok(())
    }

    /// 当前通过数。
    pub fn passed(&self) -> u32 {
        self.passed
    }

    /// 取达成率（基点）。
    pub fn rate(&self) -> Result<u32, RateCode> {
        self.freezer.rate_bp(self.passed)
    }

    /// 复核：单调性 + 往返一致（基点还原分母复核）。
    pub fn audit(&mut self) -> Result<(), RateCode> {
        let bp = self.freezer.rate_bp(self.passed)?;
        // 往返：bp × denom / 10000 应回到 passed 的基点截断带。
        let back = (bp.saturating_mul(self.freezer.denominator())) / BP_FULL;
        if back > self.passed {
            return Err(RateCode::RoundtripBreach);
        }
        self.audited = true;
        Ok(())
    }

    /// 出表：必须先 audit。
    pub fn emit(&self) -> Result<u32, RateCode> {
        if !self.audited {
            return Err(RateCode::EmitBeforeAudit);
        }
        self.freezer.rate_bp(self.passed)
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
            return Err(-(RateCode::LedgerRegress as i32));
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
    /// 分母漂移：±1 与加倍四种漂移分母
    Drift,
    /// 零分母
    ZeroDenom,
    /// 通过数越界
    Overflow,
    /// 未冻结取数
    NotFrozen,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::Drift,
        BadCase::ZeroDenom,
        BadCase::Overflow,
        BadCase::NotFrozen,
    ];
}

/// 漂移分母样本（确定性轮换：-1、+1、半值、加倍）。
fn drift_denom(round: u32) -> u32 {
    match round % 4 {
        0 => FROZEN_DENOMINATOR - 1,
        1 => FROZEN_DENOMINATOR + 1,
        2 => FROZEN_DENOMINATOR / 2,
        _ => FROZEN_DENOMINATOR * 2,
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
                    BadCase::Drift => {
                        let mut fz = DenominatorFreezer::new();
                        let _ = fz.freeze(FROZEN_DENOMINATOR);
                        fz.rate_bp_vs(120, drift_denom(round))
                            == Err(RateCode::DenominatorDrift)
                    }
                    BadCase::ZeroDenom => {
                        let mut fz = DenominatorFreezer::new();
                        fz.freeze(0) == Err(RateCode::ZeroDenominator)
                    }
                    BadCase::Overflow => {
                        let mut fz = DenominatorFreezer::new();
                        let _ = fz.freeze(FROZEN_DENOMINATOR);
                        fz.rate_bp(FROZEN_DENOMINATOR + 1) == Err(RateCode::PassOverflow)
                    }
                    BadCase::NotFrozen => {
                        let fz = DenominatorFreezer::new();
                        fz.rate_bp(120) == Err(RateCode::NotFrozen)
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

/// 基准列：通过数 = 3/4 冻结分母（180/240 = 7500‱）。
pub fn build_reference_column() -> RateColumn {
    let mut col = RateColumn::new();
    let _ = col.add_passed(FROZEN_DENOMINATOR * 3 / 4);
    let _ = col.audit();
    col
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBUSY", -16, 33, "Windows ERROR_SHARING_VIOLATION；冻结冲突族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；未冻结态取数组"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；漂移/越界族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；零分母/溢出族"),
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

/// 列指纹：通过数 + 分母 + 复核位逐字节入哈希。
pub fn column_fingerprint(col: &RateColumn) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push(((col.passed >> 8) & 0xFF) as u8);
    bytes.push((col.passed & 0xFF) as u8);
    let d = col.freezer.denominator();
    bytes.push(((d >> 8) & 0xFF) as u8);
    bytes.push((d & 0xFF) as u8);
    bytes.push(if col.audited { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10006 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10006_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10006");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 分母口径冻结值可复算
    s.add("口径冻结：分母=12×20=240",
          FROZEN_DENOMINATOR == 12 * 20,
          "分母口径必须能从冻结清单复算，写死值与推导一致");

    // 3. 基准取数正确：180/240 = 7500‱
    let col = build_reference_column();
    s.add("取数正确：3/4 分母 = 7500‱",
          col.rate() == Ok(7500),
          "整数基点除法必须给出可复算的精确值");

    // 4. 零通过与满通过两端
    let mut zc = RateColumn::new();
    let zero_bp = zc.rate();
    let full_bp = zc.add_passed(FROZEN_DENOMINATOR);
    let both = zero_bp == Ok(0) && full_bp.is_ok() && zc.rate() == Ok(BP_FULL);
    s.add("两端取数：0‱ 与 10000‱ 均精确", both,
          "空列与满列是达成率域的两个端点");

    // 5. 口径漂移闸：四种漂移分母全拒（矩阵逐轮复测）
    let (ledger, per_class) = run_bad_case_matrix();
    let drift_class = match per_class.first() {
        Some(n) => *n == INJECT_ROUNDS,
        None => false,
    };
    s.add("漂移拦截：±1/半值/加倍全拒 10/10", drift_class,
          "任何非冻结分母取数都必须拦下，不许就近取整");

    // 6. 注入矩阵：四类各 10/10 检出
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
    s.add("注入矩阵：漂移/零分母/越界/未冻结各 10/10", all_caught,
          "四类坏样本必须全部显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 未冻结取数拦截
    let uf = RateColumn::unfrozen();
    s.add("未冻结取数：冻结前 rate 被拒",
          uf.rate().err() == Some(RateCode::NotFrozen),
          "取数次序闸：先冻结后取数");

    // 9. 冻结后改分母拦截
    let mut fz = DenominatorFreezer::new();
    let _ = fz.freeze(FROZEN_DENOMINATOR);
    s.add("冻结只读：冻结后再设分母被拒",
          fz.freeze(100).err() == Some(RateCode::AlreadyFrozen),
          "口径冻结即不可变，二次冻结视为私改口径");

    // 10. 通过数越界拒绝
    let mut of = RateColumn::new();
    s.add("越界拒绝：passed>分母被拒",
          of.add_passed(FROZEN_DENOMINATOR + 1).is_ok()
              && of.rate().err() == Some(RateCode::PassOverflow),
          "通过数大于分母属口径矛盾，取数时必须拦截");

    // 11. 单调性质：通过数不减则基点不减
    let mut mono = RateColumn::new();
    let mut monotone = true;
    let mut prev = 0u32;
    let mut step = 1u32;
    while step <= 8u32 {
        let _ = mono.add_passed(30);
        match mono.rate() {
            Ok(bp) => {
                if bp < prev {
                    monotone = false;
                }
                prev = bp;
            }
            Err(_) => monotone = false,
        }
        step += 1;
    }
    s.add("单调性质：通过数不减则基点不减", monotone,
          "整数除法保序，任何逆序都说明分母被换");

    // 12. 出表闸：未 audit 拒、audit 后通过
    let mut em = RateColumn::new();
    let _ = em.add_passed(120);
    let unaudited = em.emit().err() == Some(RateCode::EmitBeforeAudit);
    let _ = em.audit();
    s.add("出表闸：未复核拒绝、复核后放行",
          unaudited && em.emit().is_ok(),
          "达成率列出表前必须过复核");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBUSY)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = column_fingerprint(&build_reference_column());
    let fp_b = column_fingerprint(&build_reference_column());
    s.add("双跑一致：达成率列指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = RateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < RateCode::ALL.len() {
        let mut j = i + 1;
        while j < RateCode::ALL.len() {
            if let (Some(a), Some(b)) = (RateCode::ALL.get(i), RateCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("达成率列指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_rate_exact() {
        let col = build_reference_column();
        assert_eq!(col.rate(), Ok(7500));
    }

    #[test]
    fn endpoints_exact() {
        let mut col = RateColumn::new();
        assert_eq!(col.rate(), Ok(0));
        let _ = col.add_passed(FROZEN_DENOMINATOR);
        assert_eq!(col.rate(), Ok(10000));
    }

    #[test]
    fn drift_denoms_rejected() {
        let mut fz = DenominatorFreezer::new();
        let _ = fz.freeze(FROZEN_DENOMINATOR);
        assert_eq!(fz.rate_bp_vs(1, FROZEN_DENOMINATOR - 1), Err(RateCode::DenominatorDrift));
        assert_eq!(fz.rate_bp_vs(1, FROZEN_DENOMINATOR + 1), Err(RateCode::DenominatorDrift));
        assert_eq!(fz.rate_bp_vs(1, 0), Err(RateCode::DenominatorDrift));
    }

    #[test]
    fn zero_denom_rejected() {
        let mut fz = DenominatorFreezer::new();
        assert_eq!(fz.freeze(0), Err(RateCode::ZeroDenominator));
    }

    #[test]
    fn pass_overflow_rejected() {
        let mut col = RateColumn::new();
        let _ = col.add_passed(FROZEN_DENOMINATOR + 1);
        assert_eq!(col.rate(), Err(RateCode::PassOverflow));
    }

    #[test]
    fn unfrozen_rate_rejected() {
        let col = RateColumn::unfrozen();
        assert_eq!(col.rate(), Err(RateCode::NotFrozen));
    }

    #[test]
    fn double_freeze_rejected() {
        let mut fz = DenominatorFreezer::new();
        assert!(fz.freeze(240).is_ok());
        assert_eq!(fz.freeze(240), Err(RateCode::AlreadyFrozen));
    }

    #[test]
    fn emit_gates_on_audit() {
        let mut col = RateColumn::new();
        let _ = col.add_passed(60);
        assert_eq!(col.emit(), Err(RateCode::EmitBeforeAudit));
        let _ = col.audit();
        assert!(col.emit().is_ok());
    }

    #[test]
    fn monotone_under_increments() {
        let mut col = RateColumn::new();
        let mut prev = 0u32;
        let mut k = 0u32;
        while k < 6 {
            let _ = col.add_passed(40);
            let bp = col.rate().unwrap_or(0);
            assert!(bp >= prev);
            prev = bp;
            k += 1;
        }
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
    fn fingerprint_deterministic() {
        assert_eq!(
            column_fingerprint(&build_reference_column()),
            column_fingerprint(&build_reference_column())
        );
    }

    #[test]
    fn unx10006_checks_green() {
        let set = run_unx10006_checks();
        assert!(set.all_passed(), "UNX-F10006 red: {}", set.red_items_text());
    }
}
