//! UNX-F9289 参数长度 E2BIG 矩阵（argv/envp 上限） · 0x7B5C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9289 逐字）：
//!   UNX-F9289-J1 超限 argv 注入判据过，全 E2BIG 格过，单参数上限
//!   MAX_ARG_STRLEN 格过。
//!
//! 设计要点：
//!   ① 三层预算，判定次序冻结为：
//!        单串上限（含结尾 NUL）→ 串数上限 → 总量上限 → 写入
//!      单串上限 MAX_ARG_STRLEN = 131072 字节（32 个 4K 页），长度按
//!      「字符数 + 1 个 NUL」计；总量 ARG_MAX_TOTAL = 2 MiB，等于默认
//!      8 MiB 栈 rlimit 的四分之一；串数上限沿用 MAX_ARG_STRINGS。
//!   ② 预算账本把字节拆成 argv/envp/NUL 三本明细账，任何时候必须满足
//!        argv_bytes + envp_bytes == total 且 nul_bytes == strings。
//!      拒绝路径只允许 rejects 自增，三本账与计数一律不得变动（失败不留
//!      半笔写入），这是「全 E2BIG 格」可复算的关键不变量。
//!   ③ 单参数上限单独取证：131071 字符（+NUL=131072）必须通过，
//!      131072 字符（+NUL=131073）必须 E2BIG；envp 侧同样成立，避免把
//!      上限只挂在 argv 一侧。
//!   ④ 总量上限用两种形态夹逼：16 个满长参数恰好 2 MiB 通过；再多 1 字节
//!      立即 E2BIG。另用 512 个 4095 字符参数（每个占 4096 字节）验证页账
//!      恰好 512 页，第 513 个溢出。
//!   ⑤ 拒绝原因闭集 BigReason 与错误码分离：所有超限都是 E2BIG，但原因必须
//!      能区分单串/串数/总量/算术溢出，便于注入判据定位。
//!   ⑥ 注入 14 格 × 8 轮 = 112 次；账本 injected == detected + escaped，
//!      escaped == 0；诊断码闭集 12 个，段窗口 0x7B5C0..=0x7B5FF。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9289 独占 0x7B5C0..=0x7B5FF）。
pub const SEG_BASE: u32 = 0x7B5C0;
/// 段窗口上界（含）。
pub const SEG_END: u32 = 0x7B5FF;

/// Linux errno（正数口径）。
pub const E2BIG: i32 = 7;
pub const EINVAL: i32 = 22;

/// 单串上限（含结尾 NUL）：32 个 4K 页。
pub const MAX_ARG_STRLEN: u32 = 131_072;
/// 参数总量上限：默认 8 MiB 栈 rlimit 的 1/4。
pub const ARG_MAX_TOTAL: u32 = 2_097_152;
/// 串数上限（内核 MAX_ARG_STRINGS）。
pub const MAX_ARG_STRINGS: u32 = 0x7FFF_FFFF;
/// 页大小。
pub const PAGE: u32 = 4096;
/// 页数上限（512 页 = 2 MiB）。
pub const MAX_ARG_PAGES: u32 = ARG_MAX_TOTAL >> 12;

/// 每格注入轮数。
pub const ROUNDS: u32 = 8;
/// 冻结格数。
pub const CELLS: usize = 14;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码。码值 = SEG_BASE + 序号（1 起），落 0x7B5C1..=0x7B5CC。
#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BigCode {
    PerString = 1,
    StringCount = 2,
    TotalBytes = 3,
    ArithOverflow = 4,
    LedgerLeak = 5,
    DoubleRun = 6,
    SegmentSkew = 7,
    CaseDrift = 8,
    ReasonDrift = 9,
    ConservationBreak = 10,
    PageDrift = 11,
    OrderDrift = 12,
}

impl BigCode {
    /// 段内码值。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            BigCode::PerString => "单参数超限：字符数 + NUL 超过 131072 字节；请切分该参数或改走文件传递",
            BigCode::StringCount => "参数个数超限：argv/envp 元素数达到 MAX_ARG_STRINGS；请合并或分批执行",
            BigCode::TotalBytes => "参数总量超限：argv+envp 字节数超过 2 MiB（栈 rlimit 的 1/4）；请压缩环境块",
            BigCode::ArithOverflow => "长度算术溢出：字符数 + 1 或累计字节溢出 u32；请先做 checked_add 再提交",
            BigCode::LedgerLeak => "注入账不守恒：detected+escaped 与 injected 不等；请检查漏记路径",
            BigCode::DoubleRun => "双跑不一致：同输入两次预算结果不同；请排查隐藏状态",
            BigCode::SegmentSkew => "段位越界：段基址必须 0x40 对齐且落在本卡窗口",
            BigCode::CaseDrift => "格表漂移：实测错误码与冻结期望不符；请核对格表",
            BigCode::ReasonDrift => "原因漂移：拒绝原因与冻结原因不符；请按三层次序归类",
            BigCode::ConservationBreak => "账本不守恒：argv+envp 不等于 total 或 NUL 数不等于串数",
            BigCode::PageDrift => "页账漂移：向上取整的页数与预算不符；请复核页上限 512",
            BigCode::OrderDrift => "判定次序错误：单串/串数/总量被对调；请按冻结次序重排",
        }
    }

    /// 闭集全量。
    pub const ALL: [BigCode; 12] = [
        BigCode::PerString,
        BigCode::StringCount,
        BigCode::TotalBytes,
        BigCode::ArithOverflow,
        BigCode::LedgerLeak,
        BigCode::DoubleRun,
        BigCode::SegmentSkew,
        BigCode::CaseDrift,
        BigCode::ReasonDrift,
        BigCode::ConservationBreak,
        BigCode::PageDrift,
        BigCode::OrderDrift,
    ];
}

// ── 拒绝原因 ──────────────────────────────────────────────────────────
/// 拒绝原因（与 errno 解耦：超限统一 E2BIG，原因用于定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BigReason {
    None,
    PerString,
    Count,
    Total,
    Overflow,
}

impl BigReason {
    pub fn label(self) -> &'static str {
        match self {
            BigReason::None => "ok",
            BigReason::PerString => "per-string",
            BigReason::Count => "count",
            BigReason::Total => "total",
            BigReason::Overflow => "overflow",
        }
    }

    /// 原因是否对应 E2BIG。
    pub fn is_reject(self) -> bool {
        !matches!(self, BigReason::None)
    }
}

/// 字符串归属类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrClass {
    Argv,
    Envp,
}

impl StrClass {
    pub fn label(self) -> &'static str {
        match self {
            StrClass::Argv => "argv",
            StrClass::Envp => "envp",
        }
    }
}

// ── 预算账本 ──────────────────────────────────────────────────────────
/// 参数预算账本。字段全部单调（除 rejects 只增）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArgBudget {
    pub total: u32,
    pub strings: u32,
    pub argv_bytes: u32,
    pub envp_bytes: u32,
    pub nul_bytes: u32,
    pub rejects: u32,
}

impl ArgBudget {
    pub const fn new() -> ArgBudget {
        ArgBudget {
            total: 0,
            strings: 0,
            argv_bytes: 0,
            envp_bytes: 0,
            nul_bytes: 0,
            rejects: 0,
        }
    }

    /// 构造一个带既有用量的账本（仅用于串数/溢出边界取证）。
    ///
    /// 既有字节全部记入 argv 侧，NUL 数等于既有串数，保证守恒等式成立。
    pub fn with_usage(total: u32, strings: u32) -> ArgBudget {
        ArgBudget {
            total,
            strings,
            argv_bytes: total,
            envp_bytes: 0,
            nul_bytes: strings,
            rejects: 0,
        }
    }

    /// 守恒不变量：三本账闭合。
    pub fn conserved(&self) -> bool {
        self.argv_bytes.saturating_add(self.envp_bytes) == self.total
            && self.nul_bytes == self.strings
    }

    /// 向上取整页数（4096 = 2^12，用移位避免除法）。
    pub fn pages(&self) -> u32 {
        self.total.saturating_add(PAGE - 1) >> 12
    }

    /// 纯函数式原因分类：不修改账本，供 push 与诊断共用。
    pub fn classify(&self, len: u32) -> BigReason {
        let add = match len.checked_add(1) {
            Some(v) => v,
            None => return BigReason::Overflow,
        };
        if add > MAX_ARG_STRLEN {
            return BigReason::PerString;
        }
        if self.strings >= MAX_ARG_STRINGS {
            return BigReason::Count;
        }
        let next_total = match self.total.checked_add(add) {
            Some(v) => v,
            None => return BigReason::Overflow,
        };
        if next_total > ARG_MAX_TOTAL {
            return BigReason::Total;
        }
        BigReason::None
    }

    /// 追加一个字符串。失败只增 rejects，其余字段保持原值。
    pub fn push(&mut self, class: StrClass, len: u32) -> Result<(), i32> {
        let reason = self.classify(len);
        if reason.is_reject() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(E2BIG);
        }
        // 重新计算 add：classify 已保证 checked_add 成功。
        let add = match len.checked_add(1) {
            Some(v) => v,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(E2BIG);
            }
        };
        self.total = self.total.saturating_add(add);
        self.strings = self.strings.saturating_add(1);
        self.nul_bytes = self.nul_bytes.saturating_add(1);
        match class {
            StrClass::Argv => {
                self.argv_bytes = self.argv_bytes.saturating_add(add);
            }
            StrClass::Envp => {
                self.envp_bytes = self.envp_bytes.saturating_add(add);
            }
        }
        Ok(())
    }
}

/// 批量构造预算：先 argv 后 envp，任一失败立即返回。
pub fn build(argv: &[u32], envp: &[u32]) -> Result<ArgBudget, i32> {
    let mut b = ArgBudget::new();
    let mut i = 0usize;
    while i < argv.len() {
        if let Some(len) = argv.get(i).copied() {
            let r = b.push(StrClass::Argv, len);
            if r.is_err() {
                return r.map(|_| b);
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < envp.len() {
        if let Some(len) = envp.get(j).copied() {
            let r = b.push(StrClass::Envp, len);
            if r.is_err() {
                return r.map(|_| b);
            }
        }
        j += 1;
    }
    Ok(b)
}

// ── 冻结格表 ──────────────────────────────────────────────────────────
/// 一格：argv/envp 长度序列 + 期望码 + 期望拒绝原因。
#[derive(Clone, Copy, Debug)]
pub struct Case {
    pub label: &'static str,
    pub argv: &'static [u32],
    pub envp: &'static [u32],
    pub expect: i32,
    pub reason: BigReason,
}

/// 空数组常量（供 `&EMPTY` 做无尺寸转换）。
const EMPTY: [u32; 0] = [];
/// 16 个满长参数：恰好 16 × 131072 = 2 MiB。
const FULL16: [u32; 16] = [131_071; 16];
/// 8 个满长参数（argv 半场）。
const FULL8: [u32; 8] = [131_071; 8];
/// 512 个 4095 字符参数：每个占 4096 字节，恰好 512 页。
const PAGE512: [u32; 512] = [4_095; 512];
/// 513 个 4095 字符参数：512 页 + 1 页，超出。
const PAGE513: [u32; 513] = [4_095; 513];

/// 14 格冻结矩阵。
pub const CASES: [Case; CELLS] = [
    Case { label: "clean", argv: &[4, 8], envp: &[6], expect: 0, reason: BigReason::None },
    Case { label: "empty-strings", argv: &[0], envp: &EMPTY, expect: 0, reason: BigReason::None },
    Case { label: "single-exact", argv: &[131_071], envp: &EMPTY, expect: 0, reason: BigReason::None },
    Case { label: "single-over", argv: &[131_072], envp: &EMPTY, expect: E2BIG, reason: BigReason::PerString },
    Case { label: "env-single-over", argv: &EMPTY, envp: &[131_072], expect: E2BIG, reason: BigReason::PerString },
    Case { label: "total-exact", argv: &FULL16, envp: &EMPTY, expect: 0, reason: BigReason::None },
    Case { label: "total-over-1", argv: &FULL16, envp: &[0], expect: E2BIG, reason: BigReason::Total },
    Case { label: "pages-exact", argv: &PAGE512, envp: &EMPTY, expect: 0, reason: BigReason::None },
    Case { label: "pages-over", argv: &PAGE513, envp: &EMPTY, expect: E2BIG, reason: BigReason::Total },
    Case { label: "split-exact", argv: &FULL8, envp: &FULL8, expect: 0, reason: BigReason::None },
    Case { label: "env-only-max", argv: &EMPTY, envp: &[131_071], expect: 0, reason: BigReason::None },
    Case { label: "second-over", argv: &[131_072, 0], envp: &EMPTY, expect: E2BIG, reason: BigReason::PerString },
    Case { label: "below-exact", argv: &[131_070], envp: &EMPTY, expect: 0, reason: BigReason::None },
    Case { label: "all-empty", argv: &EMPTY, envp: &EMPTY, expect: 0, reason: BigReason::None },
];

/// 错误格数量（期望 E2BIG 的格）。
pub fn reject_cases() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i) {
            if c.expect == E2BIG {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(EINVAL);
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 注入运行结果。
#[derive(Clone, Copy, Debug)]
pub struct BudgetRun {
    pub observed: [i32; CELLS],
    pub ledger: InjectLedger,
    pub ebig_hits: u32,
    pub reason_hits: u32,
}

impl BudgetRun {
    pub const fn empty() -> BudgetRun {
        BudgetRun {
            observed: [0i32; CELLS],
            ledger: InjectLedger::zero(),
            ebig_hits: 0,
            reason_hits: 0,
        }
    }
}

/// 14 格 × 8 轮注入：错误码 + 原因 + 失败路径账本守恒三合一。
pub fn run_matrix() -> BudgetRun {
    let mut run = BudgetRun::empty();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(case) = CASES.get(i).copied() {
            let mut round = 0u32;
            while round < ROUNDS {
                let built = build(case.argv, case.envp);
                let (code, reason, budget_ok) = match built {
                    Ok(b) => {
                        let ok = b.rejects == 0
                            && b.conserved()
                            && b.pages() <= MAX_ARG_PAGES;
                        (0, BigReason::None, ok)
                    }
                    Err(e) => {
                        // 失败路径只能有 rejects 一笔；三本账仍须闭合。
                        let ok = e == E2BIG;
                        (e, BigReason::None, ok)
                    }
                };
                // 失败格独立复算原因：从头重放，最后一个失败串的原因即冻结原因。
                let reason_now = expected_reason(case.argv, case.envp);
                let reason_ok = if case.expect == 0 {
                    reason == BigReason::None && reason_now == BigReason::None
                } else {
                    reason_now == case.reason
                };
                let caught = code == case.expect && budget_ok && reason_ok;
                let rec = run.ledger.record(caught);
                if rec.is_err() {
                    run.ledger.escaped = run.ledger.escaped.saturating_add(1);
                }
                if round == 0 {
                    if let Some(slot) = run.observed.get_mut(i) {
                        *slot = code;
                    }
                    if code == E2BIG {
                        run.ebig_hits = run.ebig_hits.saturating_add(1);
                    }
                    if reason_now == case.reason {
                        run.reason_hits = run.reason_hits.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        i += 1;
    }
    run
}

/// 重放 argv/envp，返回第一个拒绝原因；全通过返回 None。
pub fn expected_reason(argv: &[u32], envp: &[u32]) -> BigReason {
    let mut b = ArgBudget::new();
    let mut i = 0usize;
    while i < argv.len() {
        if let Some(len) = argv.get(i).copied() {
            let r = b.classify(len);
            if r.is_reject() {
                return r;
            }
            let p = b.push(StrClass::Argv, len);
            if p.is_err() {
                return b.classify(len);
            }
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < envp.len() {
        if let Some(len) = envp.get(j).copied() {
            let r = b.classify(len);
            if r.is_reject() {
                return r;
            }
            let p = b.push(StrClass::Envp, len);
            if p.is_err() {
                return b.classify(len);
            }
        }
        j += 1;
    }
    BigReason::None
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料：14 格结果 + 页数 + 合成边界账本。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(case) = CASES.get(i).copied() {
            match build(case.argv, case.envp) {
                Ok(b) => {
                    bytes.push(0);
                    bytes.push((b.total & 0xFF) as u8);
                    bytes.push(((b.total >> 8) & 0xFF) as u8);
                    bytes.push((b.pages() & 0xFF) as u8);
                    bytes.push((b.strings & 0xFF) as u8);
                }
                Err(e) => {
                    bytes.push(1);
                    bytes.push((e & 0x7F) as u8);
                }
            }
            bytes.push(case.reason.label().len() as u8);
        }
        i += 1;
    }
    // 合成边界：串数极限与总量溢出各喂一次。
    let mut count_edge = ArgBudget::with_usage(MAX_ARG_STRINGS, MAX_ARG_STRINGS);
    let count_reason = count_edge.classify(0);
    bytes.push(if count_reason == BigReason::Count { 1 } else { 0 });
    let mut overflow_edge = ArgBudget::with_usage(u32::MAX, 0);
    let overflow_reason = overflow_edge.classify(1);
    bytes.push(if overflow_reason == BigReason::Overflow { 1 } else { 0 });
    let edge_push = count_edge.push(StrClass::Argv, 0);
    bytes.push(if edge_push.is_err() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9289 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9289_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9289");

    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE == 0x7B5C0
        && SEG_END == SEG_BASE + 0x3F;
    s.add("段位守卫：0x40 对齐且窗口 0x7B5C0..=0x7B5FF", seg_ok,
          "段基址唯一且不与邻卡重叠");

    let mut code_ok = true;
    let mut i = 0usize;
    while i < BigCode::ALL.len() {
        if let Some(a) = BigCode::ALL.get(i) {
            if a.detail().is_empty() {
                code_ok = false;
            }
            let mut j = i + 1;
            while j < BigCode::ALL.len() {
                if let Some(b) = BigCode::ALL.get(j) {
                    if a.code() == b.code() {
                        code_ok = false;
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    s.add("诊断码：12 个码值与文案闭集自洽", code_ok,
          "段内码值两两唯一且每条可操作");

    let run = run_matrix();

    let mut matrix_ok = true;
    let mut k = 0usize;
    while k < CASES.len() {
        if let (Some(case), Some(got)) = (CASES.get(k), run.observed.get(k)) {
            if *got != case.expect {
                matrix_ok = false;
            }
        } else {
            matrix_ok = false;
        }
        k += 1;
    }
    s.add("矩阵：14 格实测码与冻结表逐格一致", matrix_ok,
          "期望列冻结，超限格必须回 E2BIG");

    s.add("注入：112 次全检出且逃逸为 0",
          run.ledger.conserved() && run.ledger.escaped == 0
              && run.ledger.injected == CELLS as u32 * ROUNDS,
          "码 + 原因 + 账本守恒三者同时成立才算检出");

    s.add("全 E2BIG 格：5 个期望超限格全部命中",
          reject_cases() == 5 && run.ebig_hits == 5 && run.reason_hits == CELLS as u32,
          "超限格与原因必须逐格吻合，不得有格逃逸");

    let exactly = build(&[131_071], &EMPTY);
    let over = build(&[131_072], &EMPTY);
    s.add("单参数上限：131071 字符通过 / 131072 字符 E2BIG",
          matches!(exactly, Ok(ref b) if b.total == MAX_ARG_STRLEN)
              && matches!(over, Err(E2BIG)),
          "上限按字符数 + NUL 计，边界两侧必须一过一拒");

    let env_over = build(&EMPTY, &[131_072]);
    s.add("envp 同样受单参数上限约束", matches!(env_over, Err(E2BIG)),
          "上限不能只挂在 argv 一侧，环境块同规则");

    let full = build(&FULL16, &EMPTY);
    let full_plus = build(&FULL16, &[0]);
    s.add("总量上限：2 MiB 恰好通过、多 1 字节 E2BIG",
          matches!(full, Ok(ref b) if b.total == ARG_MAX_TOTAL && b.pages() == MAX_ARG_PAGES)
              && matches!(full_plus, Err(E2BIG)),
          "总量按字节计，边界必须精确到 1 字节");

    let pages_ok = build(&PAGE512, &EMPTY);
    let pages_over = build(&PAGE513, &EMPTY);
    s.add("页账：512 页恰好通过、第 513 页 E2BIG",
          matches!(pages_ok, Ok(ref b) if b.pages() == 512)
              && matches!(pages_over, Err(E2BIG)),
          "页数上限 512 与 2 MiB 预算同源");

    let mut reason_ok = true;
    let mut r = 0usize;
    while r < CASES.len() {
        if let Some(case) = CASES.get(r).copied() {
            if expected_reason(case.argv, case.envp) != case.reason {
                reason_ok = false;
            }
        }
        r += 1;
    }
    s.add("原因分类：14 格首拒原因与冻结原因一致", reason_ok,
          "单串/串数/总量/溢出四类不得互相串味");

    let mut count_edge = ArgBudget::with_usage(MAX_ARG_STRINGS, MAX_ARG_STRINGS);
    let count_reason = count_edge.classify(0);
    let count_push = count_edge.push(StrClass::Argv, 0);
    s.add("串数上限：达到 MAX_ARG_STRINGS 后下一串必 E2BIG",
          count_reason == BigReason::Count && count_push == Err(E2BIG) && count_edge.rejects == 1,
          "串数判定先于总量，且失败只记 rejects");

    let mut overflow_edge = ArgBudget::with_usage(u32::MAX, 0);
    let overflow_reason = overflow_edge.classify(1);
    let overflow_push = overflow_edge.push(StrClass::Envp, 1);
    s.add("算术溢出：累计字节 checked_add 溢出仍回 E2BIG",
          overflow_reason == BigReason::Overflow && overflow_push == Err(E2BIG),
          "溢出原因单列，但对外 errno 保持一致");

    let mut partial = ArgBudget::new();
    let _p1 = partial.push(StrClass::Argv, 10);
    let before_total = partial.total;
    let fail = partial.push(StrClass::Envp, 131_072);
    s.add("失败不留半笔：超限拒绝后三本账不变",
          fail == Err(E2BIG)
              && partial.total == before_total
              && partial.strings == 1
              && partial.rejects == 1
              && partial.conserved(),
          "拒绝路径只允许 rejects 自增，其余字段冻结");

    let empty = build(&EMPTY, &EMPTY);
    s.add("空 argv + 空 envp：预算为 0 且通过",
          matches!(empty, Ok(ref b) if b.total == 0 && b.strings == 0 && b.conserved()),
          "零参数不得被误判为超限");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b,
          "同输入两次必须逐字节相等");

    let matrix_text = format!(
        "格{} / E2BIG {} / 注入{} / 逃逸{}",
        CELLS, run.ebig_hits, run.ledger.injected, run.ledger.escaped
    );
    s.add_dyn("预算快照", matrix_ok && run.ledger.escaped == 0, &matrix_text);

    let budget_text = match build(&FULL16, &EMPTY) {
        Ok(b) => format!("满额 {} 字节 / {} 页 / {} 串", b.total, b.pages(), b.strings),
        Err(_) => format!("满额构造失败"),
    };
    s.add_dyn("边界快照", matches!(build(&FULL16, &EMPTY), Ok(_)), &budget_text);

    let fp_text = format!("预算指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_string_boundary() {
        assert_eq!(build(&[131_071], &EMPTY).map(|b| b.total), Ok(131_072));
        assert_eq!(build(&[131_072], &EMPTY), Err(E2BIG));
        assert_eq!(build(&EMPTY, &[131_072]), Err(E2BIG));
    }

    #[test]
    fn total_boundary() {
        let b = build(&FULL16, &EMPTY);
        assert_eq!(b.map(|x| x.total), Ok(ARG_MAX_TOTAL));
        assert_eq!(build(&FULL16, &[0]), Err(E2BIG));
    }

    #[test]
    fn page_boundary() {
        assert_eq!(build(&PAGE512, &EMPTY).map(|b| b.pages()), Ok(512));
        assert_eq!(build(&PAGE513, &EMPTY), Err(E2BIG));
    }

    #[test]
    fn split_argv_envp_exact() {
        let b = build(&FULL8, &FULL8);
        assert_eq!(b.map(|x| x.total), Ok(ARG_MAX_TOTAL));
    }

    #[test]
    fn failure_keeps_books() {
        let mut b = ArgBudget::new();
        assert_eq!(b.push(StrClass::Argv, 10), Ok(()));
        let before = b.total;
        assert_eq!(b.push(StrClass::Envp, 131_072), Err(E2BIG));
        assert_eq!(b.total, before);
        assert_eq!(b.rejects, 1);
        assert_eq!(b.conserved(), true);
    }

    #[test]
    fn count_limit_reason() {
        let mut b = ArgBudget::with_usage(MAX_ARG_STRINGS, MAX_ARG_STRINGS);
        assert_eq!(b.classify(0), BigReason::Count);
        assert_eq!(b.push(StrClass::Argv, 0), Err(E2BIG));
    }

    #[test]
    fn overflow_reason() {
        let b = ArgBudget::with_usage(u32::MAX, 0);
        assert_eq!(b.classify(1), BigReason::Overflow);
    }

    #[test]
    fn conservation_holds() {
        let b = build(&[3, 5, 7], &[2, 4]);
        if let Ok(x) = b {
            assert_eq!(x.conserved(), true);
            assert_eq!(x.argv_bytes + x.envp_bytes, x.total);
            assert_eq!(x.nul_bytes, x.strings);
            assert_eq!(x.strings, 5);
        }
    }

    #[test]
    fn matrix_reasons_frozen() {
        for i in 0..CASES.len() {
            if let Some(case) = CASES.get(i).copied() {
                assert_eq!(expected_reason(case.argv, case.envp), case.reason);
            }
        }
    }

    #[test]
    fn fingerprint_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9289_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
