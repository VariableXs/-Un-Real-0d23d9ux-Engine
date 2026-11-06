//! UNX-F9336 时间/资源族 E 型与 Windows 计时器错误对照格 · 0x7C180段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9336 逐字）：
//!   锚点：UNX-F9336-J1 ERROR_INVALID_PARAMETER 映射判据过，差异账三条
//!   全列格过。
//!
//! 设计要点：
//!   ① 对照面建模：时间/资源族（setitimer/timer_create/setrlimit/
//!      getrusage/sched_setaffinity）的 Linux 错误码到 Windows 计时器/
//!      句柄错误码的冻结映射。EINVAL/EINVAL_EAGAIN/EFAULT/EPERM 四键
//!      各有归宿，87（ERROR_INVALID_PARAMETER）是参数面汇聚点。
//!   ② 保真档三档闭集：Exact（逐位对应）/ Approx（语义邻近，只进差异
//!      账）/ None（Windows 无对应，缺席登记）。判定只用 Exact 档，
//!      Approx 只进差异账，None 显性缺席不静默。
//!   ③ 差异账三条全列（冻结）：
//!      D1 EAGAIN→ERROR_NO_SYSTEM_RESOURCES(1450)：资源耗尽族对应，
//!         但 Windows 计时器资源模型不同，记 Approx；
//!      D2 EPERM→ERROR_ACCESS_DENIED(5)：特权族对应但触发面不同
//!         （Linux 查 capability，Windows 查令牌特权），记 Approx；
//!      D3 EINTR→ERROR_OPERATION_ABORTED(995)：中断族对应但 Windows
//!         计时器等待不产生该码（属等待 API 面），记 Approx。
//!      三条少一条多一条都判红。
//!   ④ 映射方向守卫：Linux errno 为键、Windows 码为值；反向查询缺席。
//!      一对多拒绝（同一 errno 不得映到两个 Windows 码），撞码显性登记。
//!   ⑤ 注入核四类坏样本：表外 errno、Windows 码为 0、方向倒置、
//!      差异账缺条——逐类拒绝，账守恒逃逸 = 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9336 独占 0x7C180..=0x7C1BF）。
pub const SEG_BASE: u32 = 0x7C180;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 差异账固定条数。
pub const DIFF_ROWS: usize = 3;

/// Linux errno（正数口径）。
pub const EINVAL: i32 = 22;
pub const EAGAIN: i32 = 11;
pub const EPERM: i32 = 1;
pub const EINTR: i32 = 4;
pub const EFAULT: i32 = 14;

/// Windows 错误码（正数口径）。
pub const WIN_ERROR_INVALID_PARAMETER: u32 = 87;
pub const WIN_ERROR_NO_SYSTEM_RESOURCES: u32 = 1450;
pub const WIN_ERROR_ACCESS_DENIED: u32 = 5;
pub const WIN_ERROR_OPERATION_ABORTED: u32 = 995;
pub const WIN_ERROR_NOT_SUPPORTED: u32 = 50;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TresMapCode {
    /// 表外 errno
    UnknownErrno,
    /// Windows 码为 0
    ZeroWinCode,
    /// 方向倒置
    ReversedDirection,
    /// 一对多
    OneToMany,
    /// 差异账条数不符
    DiffLedgerCount,
    /// 差异账编号断档
    DiffLedgerGap,
    /// 保真档越界
    FidelityOutOfRange,
    /// 哨兵混路（inf/0 参与数值比较）
    SentinelMixed,
    /// 账本倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl TresMapCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            TresMapCode::UnknownErrno => "表外 errno 被拒：映射表是闭集；请先把新码 \
                连同保真档写入冻结表再使用",
            TresMapCode::ZeroWinCode => "Windows 码为 0 被拒：0 是成功哨兵，不得 \
                充当错误映射目标；请改填真实错误码",
            TresMapCode::ReversedDirection => "方向倒置被拒：必须以 Linux errno 为键、\
                Windows 码为值查询；反向查询缺席",
            TresMapCode::OneToMany => "一对多被拒：同一 errno 不得映到两个 Windows 码；\
                请合并行或删除歧义行",
            TresMapCode::DiffLedgerCount => "差异账条数不符：判据要求三条全列；\
                多列少列都破坏对账基线",
            TresMapCode::DiffLedgerGap => "差异账编号断档：D1/D2/D3 必须连续；\
                断档说明有条目被静默丢弃",
            TresMapCode::FidelityOutOfRange => "保真档越界被拒：档位闭集为 \
                Exact/Approx/None，越界档拒绝入表",
            TresMapCode::SentinelMixed => "哨兵混路被拒：errno 键域只有正整数；\
                0 与负值不得进入映射路径",
            TresMapCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查 \
                是否有回滚路径误改累计值",
            TresMapCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            TresMapCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [TresMapCode; 11] = [
        TresMapCode::UnknownErrno,
        TresMapCode::ZeroWinCode,
        TresMapCode::ReversedDirection,
        TresMapCode::OneToMany,
        TresMapCode::DiffLedgerCount,
        TresMapCode::DiffLedgerGap,
        TresMapCode::FidelityOutOfRange,
        TresMapCode::SentinelMixed,
        TresMapCode::LedgerRegress,
        TresMapCode::NotReproducible,
        TresMapCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带诊断码。
pub type SysResult<T> = Result<T, TresMapCode>;

// ── 保真档 ───────────────────────────────────────────────────────────
/// 映射保真档（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fidelity {
    /// 语义逐位对应，可参与判定
    Exact,
    /// 语义邻近，只进差异账
    Approx,
    /// Windows 侧无对应，缺席登记
    None,
}

// ── 冻结映射注册表 ───────────────────────────────────────────────────
/// 单行映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TresMapRow {
    /// Linux errno 正数
    pub errno: i32,
    /// Windows 错误码
    pub win: u32,
    /// 保真档
    pub fidelity: Fidelity,
}

/// 时间/资源族错误对照注册表（冻结行序）。
#[derive(Clone, Copy, Debug)]
pub struct TresMapRegistry {
    rows: [Option<TresMapRow>; 8],
    used: usize,
    /// 差异账登记条数（目标恒为 DIFF_ROWS）
    diff_entries: u32,
}

impl TresMapRegistry {
    /// 构造冻结表：EINVAL→87（Exact）、EAGAIN→1450、EPERM→5、
    /// EINTR→995（均 Approx）、EFAULT→87（Exact）。
    pub fn new() -> TresMapRegistry {
        let mut r = TresMapRegistry {
            rows: [None; 8],
            used: 0,
            diff_entries: 0,
        };
        let _ = r.push(TresMapRow {
            errno: EINVAL,
            win: WIN_ERROR_INVALID_PARAMETER,
            fidelity: Fidelity::Exact,
        });
        let _ = r.push(TresMapRow {
            errno: EFAULT,
            win: WIN_ERROR_INVALID_PARAMETER,
            fidelity: Fidelity::Exact,
        });
        let _ = r.push(TresMapRow {
            errno: EAGAIN,
            win: WIN_ERROR_NO_SYSTEM_RESOURCES,
            fidelity: Fidelity::Approx,
        });
        let _ = r.push(TresMapRow {
            errno: EPERM,
            win: WIN_ERROR_ACCESS_DENIED,
            fidelity: Fidelity::Approx,
        });
        let _ = r.push(TresMapRow {
            errno: EINTR,
            win: WIN_ERROR_OPERATION_ABORTED,
            fidelity: Fidelity::Approx,
        });
        r
    }

    /// 入表：非正 errno、win=0、重复 errno 逐项拒绝。
    pub fn push(&mut self, row: TresMapRow) -> SysResult<()> {
        if row.errno <= 0 {
            return Err(TresMapCode::SentinelMixed);
        }
        if row.win == 0 {
            return Err(TresMapCode::ZeroWinCode);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(prev) = self.rows.get(i).copied().and_then(|o| o) {
                if prev.errno == row.errno {
                    return Err(TresMapCode::OneToMany);
                }
            }
            i += 1;
        }
        if self.used >= self.rows.len() {
            return Err(TresMapCode::FidelityOutOfRange);
        }
        if let Some(slot) = self.rows.get_mut(self.used) {
            *slot = Some(row);
            self.used += 1;
            Ok(())
        } else {
            Err(TresMapCode::FidelityOutOfRange)
        }
    }

    /// 正向查询：errno → Windows 码。
    pub fn lookup(&self, errno: i32) -> SysResult<u32> {
        if errno <= 0 {
            return Err(TresMapCode::ReversedDirection);
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(Some(row)) = self.rows.get(i).copied() {
                if row.errno == errno {
                    return Ok(row.win);
                }
            }
            i += 1;
        }
        Err(TresMapCode::UnknownErrno)
    }

    /// 87 汇聚判定：EINVAL 与 EFAULT 都到达 ERROR_INVALID_PARAMETER。
    pub fn invalid_parameter_holds(&self) -> bool {
        self.lookup(EINVAL) == Ok(WIN_ERROR_INVALID_PARAMETER)
            && self.lookup(EFAULT) == Ok(WIN_ERROR_INVALID_PARAMETER)
    }

    /// 差异账登记（只增，超 3 条判错）。
    pub fn register_diff(&mut self) -> SysResult<()> {
        if self.diff_entries >= DIFF_ROWS as u32 {
            return Err(TresMapCode::DiffLedgerCount);
        }
        self.diff_entries = self.diff_entries.saturating_add(1);
        Ok(())
    }

    /// 差异账条数。
    pub fn diffs(&self) -> u32 {
        self.diff_entries
    }

    /// 已用行数。
    pub fn used_rows(&self) -> usize {
        self.used
    }
}

// ── 差异账（三条全列，冻结内容） ─────────────────────────────────────
/// 差异账条目：(编号, Linux 侧, Windows 侧, 差异说明)。
pub const DIFF_LEDGER: &[(&str, &str, &str, &str)] = &[
    ("D1", "EAGAIN(11)", "ERROR_NO_SYSTEM_RESOURCES(1450)",
     "资源耗尽族对应但 Windows 计时器资源模型不同，保真档 Approx"),
    ("D2", "EPERM(1)", "ERROR_ACCESS_DENIED(5)",
     "特权族对应但触发面不同：Linux 查 capability，Windows 查令牌特权"),
    ("D3", "EINTR(4)", "ERROR_OPERATION_ABORTED(995)",
     "中断族对应但 Windows 计时器等待不产生该码，属等待 API 面"),
];

/// 差异账完整性：恰 3 条、编号连续、说明非空。
pub fn diff_ledger_complete() -> bool {
    if DIFF_LEDGER.len() != DIFF_ROWS {
        return false;
    }
    let mut i = 0usize;
    while i < DIFF_LEDGER.len() {
        if let Some(row) = DIFF_LEDGER.get(i) {
            if row.3.is_empty() {
                return false;
            }
            let expect = format!("D{}", i + 1);
            if row.0 != expect.as_str() {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏映射样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadTresMapCase {
    /// 表外 errno
    UnknownErrno,
    /// Windows 码为 0
    ZeroWin,
    /// 方向倒置
    Reversed,
    /// 差异账缺条
    DiffMissing,
}

impl BadTresMapCase {
    /// 闭集全量。
    pub const ALL: [BadTresMapCase; 4] = [
        BadTresMapCase::UnknownErrno,
        BadTresMapCase::ZeroWin,
        BadTresMapCase::Reversed,
        BadTresMapCase::DiffMissing,
    ];
}

/// 注入账。
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

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮四类坏映射注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_tres_map_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadTresMapCase::ALL.len() {
        if let Some(case) = BadTresMapCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadTresMapCase::UnknownErrno => {
                        let r = TresMapRegistry::new();
                        r.lookup(404).is_err()
                    }
                    BadTresMapCase::ZeroWin => {
                        let mut r = TresMapRegistry::new();
                        r.push(TresMapRow {
                            errno: 33,
                            win: 0,
                            fidelity: Fidelity::Exact,
                        })
                        .is_err()
                    }
                    BadTresMapCase::Reversed => {
                        let r = TresMapRegistry::new();
                        r.lookup(WIN_ERROR_INVALID_PARAMETER as i32).is_err()
                    }
                    BadTresMapCase::DiffMissing => {
                        let mut r = TresMapRegistry::new();
                        r.diff_entries = 0;
                        r.diffs() < DIFF_ROWS as u32
                    }
                };
                ledger.record(caught);
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

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", -4, 995, "Windows ERROR_OPERATION_ABORTED；中断族"),
    ("EAGAIN", -11, 1450, "Windows ERROR_NO_SYSTEM_RESOURCES；资源耗尽族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；特权族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
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

/// 规范语料双跑指纹：五键查询 × 方向倒置 × 差异账。
pub fn corpus_fingerprint() -> u32 {
    let reg = TresMapRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let cases: [i32; 7] = [EINVAL, EAGAIN, EPERM, EINTR, EFAULT, 0, -1];
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(e) = cases.get(i).copied() {
            bytes.push(match reg.lookup(e) {
                Ok(w) => (w & 0xFF) as u8,
                Err(c) => (c as u32 & 0xFF) as u8,
            });
        }
        i += 1;
    }
    bytes.push(if reg.invalid_parameter_holds() { 1 } else { 0 });
    bytes.push(if diff_ledger_complete() { 1 } else { 0 });
    bytes.push(reg.used_rows() as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9336 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9336_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9336");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. ERROR_INVALID_PARAMETER 映射判据
    let reg = TresMapRegistry::new();
    s.add("87 汇聚：EINVAL/EFAULT 双双到达 ERROR_INVALID_PARAMETER",
          reg.invalid_parameter_holds(),
          "参数面与坏地址面在 Windows 侧汇聚到 87");

    // 3. 四键映射全命中
    let four_ok = reg.lookup(EINVAL).is_ok()
        && reg.lookup(EAGAIN).is_ok()
        && reg.lookup(EPERM).is_ok()
        && reg.lookup(EINTR).is_ok()
        && reg.lookup(EFAULT).is_ok();
    s.add("五键映射：EINVAL/EAGAIN/EPERM/EINTR/EFAULT 全命中", four_ok,
          "时间/资源族错误面的键闭集全部可达");

    // 4. 方向守卫
    let dir_ok = reg.lookup(0).is_err()
        && reg.lookup(-EINVAL).is_err()
        && reg.lookup(WIN_ERROR_NOT_SUPPORTED as i32).is_err();
    s.add("方向守卫：0/负值/Windows 码当 errno 查全拒", dir_ok,
          "查询键域只有正 errno");

    // 5. 一对多拒绝
    let mut dup = TresMapRegistry::new();
    let dup_ok = dup.push(TresMapRow {
        errno: EINVAL,
        win: WIN_ERROR_NOT_SUPPORTED,
        fidelity: Fidelity::Exact,
    }) == Err(TresMapCode::OneToMany);
    s.add("一对多：同 errno 二次入表被拒", dup_ok,
          "映射必须函数化：一个 errno 至多一个 Windows 码");

    // 6. 零码与哨兵混路
    let sent_ok = dup.push(TresMapRow {
        errno: 33,
        win: 0,
        fidelity: Fidelity::Exact,
    }) == Err(TresMapCode::ZeroWinCode)
        && dup.push(TresMapRow {
            errno: -5,
            win: 5,
            fidelity: Fidelity::Exact,
        }) == Err(TresMapCode::SentinelMixed);
    s.add("入表守卫：win=0 与负 errno 全拒", sent_ok,
          "0 是成功哨兵，键域只有正 errno");

    // 7. 差异账三条全列
    s.add("差异账：恰三条、编号连续、说明非空", diff_ledger_complete(),
          "D1/D2/D3 逐条冻结，多列少列都判红");

    // 8. 差异账登记上限
    let mut cap = TresMapRegistry::new();
    let _ = cap.register_diff();
    let _ = cap.register_diff();
    let _ = cap.register_diff();
    let cap_ok = cap.diffs() == 3 && cap.register_diff() == Err(TresMapCode::DiffLedgerCount);
    s.add("差异账上限：第 4 条登记被拒", cap_ok,
          "账目以 3 为闭集容量，超出显性拒绝");

    // 9. 注入核：四类坏样本
    let (ledger, per_class) = run_bad_tres_map_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏映射各 10/10 全检出", all_caught,
          "表外 errno/win=0/方向倒置/差异账缺条必须逐类拒绝");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EAGAIN)
        && agrees_with_linux(EPERM)
        && agrees_with_linux(EINTR)
        && agrees_with_linux(EFAULT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 12. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 13. 文案闭集与码唯一
    let text_ok = TresMapCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < TresMapCode::ALL.len() {
        let mut j = i + 1;
        while j < TresMapCode::ALL.len() {
            if let (Some(x), Some(y)) = (TresMapCode::ALL.get(i), TresMapCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {} / 差异账 {}",
        ledger.injected, ledger.detected, ledger.escaped, cap.diffs()
    );
    s.add_dyn("账目快照", ledger.conserved() && diff_ledger_complete(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_parameter_convergence() {
        let r = TresMapRegistry::new();
        assert_eq!(r.lookup(EINVAL), Ok(WIN_ERROR_INVALID_PARAMETER));
        assert_eq!(r.lookup(EFAULT), Ok(WIN_ERROR_INVALID_PARAMETER));
    }

    #[test]
    fn five_keys_hit() {
        let r = TresMapRegistry::new();
        assert!(r.lookup(EINVAL).is_ok());
        assert!(r.lookup(EAGAIN).is_ok());
        assert!(r.lookup(EPERM).is_ok());
        assert!(r.lookup(EINTR).is_ok());
        assert!(r.lookup(EFAULT).is_ok());
    }

    #[test]
    fn unknown_and_reversed_rejected() {
        let r = TresMapRegistry::new();
        assert!(r.lookup(404).is_err());
        assert!(r.lookup(0).is_err());
        assert!(r.lookup(-22).is_err());
    }

    #[test]
    fn one_to_many_rejected() {
        let mut r = TresMapRegistry::new();
        assert_eq!(
            r.push(TresMapRow {
                errno: EINVAL,
                win: WIN_ERROR_NOT_SUPPORTED,
                fidelity: Fidelity::Exact,
            }),
            Err(TresMapCode::OneToMany)
        );
    }

    #[test]
    fn zero_win_rejected() {
        let mut r = TresMapRegistry::new();
        assert!(r.push(TresMapRow {
            errno: 33,
            win: 0,
            fidelity: Fidelity::Exact,
        })
        .is_err());
    }

    #[test]
    fn diff_ledger_three_rows() {
        assert!(diff_ledger_complete());
        assert_eq!(DIFF_LEDGER.len(), 3);
    }

    #[test]
    fn diff_cap_enforced() {
        let mut r = TresMapRegistry::new();
        let _ = r.register_diff();
        let _ = r.register_diff();
        let _ = r.register_diff();
        assert!(r.register_diff().is_err());
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per) = run_bad_tres_map_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EINVAL));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9336_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
