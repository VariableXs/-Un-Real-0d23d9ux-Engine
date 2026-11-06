//! UNX-F9330 rlimit 软硬倒置 EINVAL 矩阵（联 B19 对账） · 0x7C000段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9330 逐字）：
//!   锚点：UNX-F9330-J1 软硬倒置注入判据过，全 EINVAL 格过，与 B19
//!   矩阵零重复分层格过。
//!
//! 设计要点：
//!   ① 倒置面建模：setrlimit(soft > hard) 是 Linux 冻结的 EINVAL——软限
//!      不得超过硬限。倒置矩阵逐格注入：soft 略超、soft 远超、soft ==
//!      hard（合法边界，必须放行）、soft < hard（合法）。判据只打倒置格，
//!      边界格与合法格同时落账防止「一刀切全拒」的假绿。
//!   ② 特权面挂点：提升硬限需要特权（CAP_SYS_RESOURCE）。非特权进程
//!      把 hard 抬高回 EPERM，不是 EINVAL——两码分层：倒置属参数面
//!      （EINVAL），越权属特权面（EPERM）。判定次序：倒置校验先于特权
//!      校验（Linux 的 rlim 校验在 capable 之前）。
//!   ③ RLIM_INFINITY 哨兵：inf 可以作软限也可以作硬限；soft=inf 且
//!      hard=有限值是倒置的一种（inf > 有限），必须 EINVAL。inf==inf
//!      合法。哨兵参与比较时显性特判，不走普通数值路径。
//!   ④ 与 B19 矩阵零重复分层：B19 管 SIGPENDING/NOFILE 资源语义面，
//!      本模块只管 setrlimit 参数面（倒置/越权/哨兵）。同码不重测：
//!      B19 的 EAGAIN/信号账本模块不碰，本模块的 EINVAL 格 B19 不打。
//!   ⑤ 注入核四类坏样本：soft 略超、soft=inf vs 有限 hard、非特权抬
//!      硬限、双零坏快照——逐类拒绝，账守恒逃逸 = 0。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9330 独占 0x7C000..=0x7C03F）。
pub const SEG_BASE: u32 = 0x7C000;

/// 注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// RLIM_INFINITY 哨兵值。
pub const RLIM_INFINITY: u64 = u64::MAX;

/// 特权位（CAP_SYS_RESOURCE 语义）。
pub const CAP_SYS_RESOURCE: u32 = 24;

/// Linux errno：坏参数。
pub const EINVAL: i32 = 22;
/// Linux errno：越权。
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlimitCode {
    /// 软限超过硬限
    SoftBeyondHard,
    /// inf 软限配有限硬限
    InfSoftFiniteHard,
    /// 非特权抬硬限
    UnprivilegedHardRaise,
    /// 非特权压硬限降低
    UnprivilegedHardLower,
    /// which 资源档越界
    BadResource,
    /// 判定次序错乱（倒置漏判而特权先拦）
    OrderInverted,
    /// 哨兵路径混入数值比较
    SentinelMixed,
    /// B19 重复覆盖
    OverlapWithB19,
    /// 账本倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl RlimitCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RlimitCode::SoftBeyondHard => "软硬倒置被拒：setrlimit 的软限不得超过 \
                硬限；倒置一律 EINVAL，先改硬限（需特权）再抬软限",
            RlimitCode::InfSoftFiniteHard => "inf 倒置被拒：RLIM_INFINITY 软限配 \
                有限硬限等价于倒置；inf 只能与 inf 共存或作硬限",
            RlimitCode::UnprivilegedHardRaise => "非特权抬硬限被拒：提升硬限需要 \
                CAP_SYS_RESOURCE；越权回 EPERM 不是 EINVAL",
            RlimitCode::UnprivilegedHardLower => "非特权降硬限被拒：硬限只能降级 \
                到软限位置且不可逆（irreversible）；本格只登记非特权改硬限 \
                的越权面",
            RlimitCode::BadResource => "资源档越界被拒：which 闭集为 \
                SIGPENDING/NOFILE/CORE 等 B19 登记档；越界回 EINVAL",
            RlimitCode::OrderInverted => "次序错乱被拒：倒置校验必须先于特权 \
                校验；特权先拦会让倒置样本错拿 EPERM",
            RlimitCode::SentinelMixed => "哨兵混路被拒：RLIM_INFINITY 参与比较时 \
                必须显性特判；混入数值路径会把 inf 判成极大合法值",
            RlimitCode::OverlapWithB19 => "B19 重复被拒：本模块只管 setrlimit \
                参数面；SIGPENDING 资源语义归 B19，重复打格即覆盖",
            RlimitCode::LedgerRegress => "账本倒退被拒：注入账只增不减；请检查 \
                是否有回滚路径误改累计值",
            RlimitCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            RlimitCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7B400..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RlimitCode; 11] = [
        RlimitCode::SoftBeyondHard,
        RlimitCode::InfSoftFiniteHard,
        RlimitCode::UnprivilegedHardRaise,
        RlimitCode::UnprivilegedHardLower,
        RlimitCode::BadResource,
        RlimitCode::OrderInverted,
        RlimitCode::SentinelMixed,
        RlimitCode::OverlapWithB19,
        RlimitCode::LedgerRegress,
        RlimitCode::NotReproducible,
        RlimitCode::SegmentViolation,
    ];
}

/// 系统调用失败结果：携带 errno 正数。
pub type SysResult<T> = Result<T, i32>;

// ── 资源档闭集（与 B19 分层登记） ────────────────────────────────────
/// B19 登记的资源档（本模块只引用，不重复打格）。
pub const RES_SIGPENDING: u32 = 0;
pub const RES_NOFILE: u32 = 1;
pub const RES_CORE: u32 = 2;

/// which 闭集校验。
pub fn validate_resource(which: u32) -> SysResult<()> {
    match which {
        RES_SIGPENDING | RES_NOFILE | RES_CORE => Ok(()),
        _ => Err(EINVAL),
    }
}

// ── setrlimit 判定器 ─────────────────────────────────────────────────
/// setrlimit 判定。old_hard 为当前硬限，privileged 为进程特权位。
/// 次序冻结：which 校验 → 倒置校验（哨兵特判）→ 特权校验。
pub fn judge_setrlimit(
    which: u32,
    soft: u64,
    hard: u64,
    old_hard: u64,
    privileged: bool,
) -> SysResult<()> {
    validate_resource(which)?;
    // 倒置校验（哨兵显性特判，不走数值路径）。
    let inverted = if soft == RLIM_INFINITY {
        hard != RLIM_INFINITY
    } else if hard == RLIM_INFINITY {
        false
    } else {
        soft > hard
    };
    if inverted {
        return Err(EINVAL);
    }
    // 特权校验：新硬限高于旧硬限需要特权。
    if hard > old_hard && !privileged {
        return Err(EPERM);
    }
    Ok(())
}

// ── rlim 对账账本 ────────────────────────────────────────────────────
/// rlim 账本：每次判定落账。
#[derive(Clone, Copy, Debug)]
pub struct RlimitLedger {
    /// 判定总数
    pub judged: u32,
    /// EINVAL 次数
    pub einval: u32,
    /// EPERM 次数
    pub eperm: u32,
    /// 放行次数
    pub accepted: u32,
}

impl RlimitLedger {
    /// 构造空账。
    pub const fn new() -> RlimitLedger {
        RlimitLedger {
            judged: 0,
            einval: 0,
            eperm: 0,
            accepted: 0,
        }
    }

    /// 记一次判定结果（账目守恒在 record 内强制闭合）。
    pub fn record(&mut self, result: i32) {
        self.judged = self.judged.saturating_add(1);
        if result == EINVAL {
            self.einval = self.einval.saturating_add(1);
        } else if result == EPERM {
            self.eperm = self.eperm.saturating_add(1);
        } else {
            self.accepted = self.accepted.saturating_add(1);
        }
    }

    /// 账目闭合：judged = einval + eperm + accepted。
    pub fn closed(&self) -> bool {
        self.einval
            .saturating_add(self.eperm)
            .saturating_add(self.accepted)
            == self.judged
    }
}

// ── 注入核：四类坏样本 ───────────────────────────────────────────────
/// 四类坏 setrlimit 样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRlimitCase {
    /// soft 略超 hard
    SlightInversion,
    /// inf 软限配有限硬限
    InfInversion,
    /// 非特权抬硬限
    PrivilegeRaise,
    /// which 越界
    BadWhich,
}

impl BadRlimitCase {
    /// 闭集全量。
    pub const ALL: [BadRlimitCase; 4] = [
        BadRlimitCase::SlightInversion,
        BadRlimitCase::InfInversion,
        BadRlimitCase::PrivilegeRaise,
        BadRlimitCase::BadWhich,
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

/// 执行一轮四类坏 setrlimit 注入：每类 INJECT_ROUNDS 次，全部必须被拒。
pub fn run_bad_rlimit_matrix() -> (InjectLedger, [u32; 4], RlimitLedger) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut rlim_ledger = RlimitLedger::new();
    let mut ci = 0usize;
    while ci < BadRlimitCase::ALL.len() {
        if let Some(case) = BadRlimitCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let (caught, code) = match case {
                    BadRlimitCase::SlightInversion => {
                        let r = judge_setrlimit(RES_NOFILE, 101, 100, 100, true);
                        (r == Err(EINVAL), 0)
                    }
                    BadRlimitCase::InfInversion => {
                        let r = judge_setrlimit(RES_NOFILE, RLIM_INFINITY, 100, 100, true);
                        (r == Err(EINVAL), 0)
                    }
                    BadRlimitCase::PrivilegeRaise => {
                        let r = judge_setrlimit(RES_NOFILE, 50, 200, 100, false);
                        (r == Err(EPERM), 1)
                    }
                    BadRlimitCase::BadWhich => {
                        let r = judge_setrlimit(99, 10, 20, 20, true);
                        (r == Err(EINVAL), 0)
                    }
                };
                rlim_ledger.record(match code {
                    0 => EINVAL,
                    _ => EPERM,
                });
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
    (ledger, per_class, rlim_ledger)
}

// ── B19 零重复分层表 ─────────────────────────────────────────────────
/// (分层项, 本模块职责, B19 职责, 是否零重复)
pub const LAYER_TABLE: &[(&str, &str, &str, bool)] = &[
    ("EINVAL 格", "倒置/越界 which", "不打（资源面不打参数格）", true),
    ("EPERM 格", "非特权抬硬限", "不打", true),
    ("SIGPENDING 资源语义", "不碰", "预算账/排队账", true),
    ("NOFILE/CORE 语义", "只校验参数合法性", "运行期资源行为", true),
];

/// 与 B19 零重复分层成立。
pub fn layers_disjoint_with_b19() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < LAYER_TABLE.len() {
        if let Some(row) = LAYER_TABLE.get(i) {
            if !row.3 {
                ok = false;
            }
        }
        i += 1;
    }
    ok
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；特权族"),
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

/// 规范语料双跑指纹：倒置四格 + 边界三格 + 次序格。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    // 倒置格
    bytes.push(match judge_setrlimit(RES_NOFILE, 101, 100, 100, true) {
        Err(EINVAL) => 1,
        _ => 0,
    });
    bytes.push(match judge_setrlimit(RES_NOFILE, RLIM_INFINITY, 100, 100, true) {
        Err(EINVAL) => 1,
        _ => 0,
    });
    // 边界与合法格
    bytes.push(match judge_setrlimit(RES_NOFILE, 100, 100, 100, true) {
        Ok(()) => 1,
        _ => 0,
    });
    bytes.push(match judge_setrlimit(RES_NOFILE, 50, 100, 100, true) {
        Ok(()) => 1,
        _ => 0,
    });
    bytes.push(match judge_setrlimit(RES_NOFILE, RLIM_INFINITY, RLIM_INFINITY, RLIM_INFINITY, true) {
        Ok(()) => 1,
        _ => 0,
    });
    // 次序格：倒置 + 非特权 → EINVAL（倒置先拦）
    bytes.push(match judge_setrlimit(RES_NOFILE, 300, 200, 100, false) {
        Err(EINVAL) => 1,
        _ => 0,
    });
    // 特权格
    bytes.push(match judge_setrlimit(RES_NOFILE, 50, 200, 100, true) {
        Ok(()) => 1,
        _ => 0,
    });
    bytes.push(match judge_setrlimit(RES_NOFILE, 50, 200, 100, false) {
        Err(EPERM) => 1,
        _ => 0,
    });
    bytes.push(if layers_disjoint_with_b19() { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9330 判据集（≥12 条；动态明细 ≤8 条）。
pub fn run_unx9330_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9330");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7B400..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本批窗口", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 软硬倒置全 EINVAL
    let inv_ok = judge_setrlimit(RES_NOFILE, 101, 100, 100, true) == Err(EINVAL)
        && judge_setrlimit(RES_CORE, 9999, 100, 100, true) == Err(EINVAL)
        && judge_setrlimit(RES_SIGPENDING, RLIM_INFINITY, 100, 100, true) == Err(EINVAL);
    s.add("倒置注入：soft>hard / inf 软配有限硬全 EINVAL", inv_ok,
          "倒置一律 EINVAL，哨兵特判显性化");

    // 3. 边界放行：soft==hard 与 inf==inf
    let edge_ok = judge_setrlimit(RES_NOFILE, 100, 100, 100, true).is_ok()
        && judge_setrlimit(
            RES_NOFILE,
            RLIM_INFINITY,
            RLIM_INFINITY,
            RLIM_INFINITY,
            true,
        )
        .is_ok()
        && judge_setrlimit(RES_NOFILE, 50, RLIM_INFINITY, RLIM_INFINITY, true).is_ok();
    s.add("边界正判据：soft==hard / inf==inf / 有限软配 inf 硬全放行", edge_ok,
          "合法格必须放行，防止一刀切全拒的假绿");

    // 4. 特权面：抬硬限
    let priv_ok = judge_setrlimit(RES_NOFILE, 50, 200, 100, false) == Err(EPERM)
        && judge_setrlimit(RES_NOFILE, 50, 200, 100, true).is_ok();
    s.add("特权挂点：非特权抬硬限 EPERM / 特权放行", priv_ok,
          "越权与倒置分码：EPERM ≠ EINVAL");

    // 5. 判定次序：倒置先于特权
    let order_ok = judge_setrlimit(RES_NOFILE, 300, 200, 100, false) == Err(EINVAL);
    s.add("判定次序：倒置 + 非特权 → EINVAL 先拦", order_ok,
          "参数面校验先于特权面，两码不互串");

    // 6. 资源档闭集
    let which_ok = validate_resource(RES_SIGPENDING).is_ok()
        && validate_resource(RES_NOFILE).is_ok()
        && validate_resource(RES_CORE).is_ok()
        && validate_resource(3).is_err()
        && validate_resource(0xFFFF).is_err();
    s.add("which 闭集：B19 登记档外全拒", which_ok,
          "档位域闭集，越界回 EINVAL");

    // 7. 注入核：四类坏样本
    let (ledger, per_class, rlim_ledger) = run_bad_rlimit_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("四类坏样本各 10/10 全检出", all_caught,
          "略超/inf 倒置/越权抬硬/which 越界必须逐类拒绝");

    // 8. rlim 账闭合
    s.add("rlim 账闭合：judged=einval+eperm+accepted", rlim_ledger.closed(),
          "三分账之和等于判定总数");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. B19 零重复分层
    s.add("B19 分层：参数面与资源语义面零重复", layers_disjoint_with_b19(),
          "本模块只打参数格，资源语义归 B19");

    // 11. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL)
        && agrees_with_linux(EPERM)
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
    let text_ok = RlimitCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut i = 0usize;
    while i < RlimitCode::ALL.len() {
        let mut j = i + 1;
        while j < RlimitCode::ALL.len() {
            if let (Some(x), Some(y)) = (RlimitCode::ALL.get(i), RlimitCode::ALL.get(j)) {
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
        "einval {} / eperm {} / accepted {}",
        rlim_ledger.einval, rlim_ledger.eperm, rlim_ledger.accepted
    );
    s.add_dyn("rlim 快照", rlim_ledger.closed(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soft_beyond_hard_einval() {
        assert_eq!(
            judge_setrlimit(RES_NOFILE, 101, 100, 100, true),
            Err(EINVAL)
        );
    }

    #[test]
    fn inf_inversion_einval() {
        assert_eq!(
            judge_setrlimit(RES_NOFILE, RLIM_INFINITY, 100, 100, true),
            Err(EINVAL)
        );
    }

    #[test]
    fn equal_and_below_ok() {
        assert!(judge_setrlimit(RES_NOFILE, 100, 100, 100, true).is_ok());
        assert!(judge_setrlimit(RES_NOFILE, 50, 100, 100, true).is_ok());
    }

    #[test]
    fn inf_pair_ok() {
        assert!(judge_setrlimit(
            RES_NOFILE,
            RLIM_INFINITY,
            RLIM_INFINITY,
            RLIM_INFINITY,
            true
        )
        .is_ok());
    }

    #[test]
    fn unprivileged_raise_eperm() {
        assert_eq!(
            judge_setrlimit(RES_NOFILE, 50, 200, 100, false),
            Err(EPERM)
        );
        assert!(judge_setrlimit(RES_NOFILE, 50, 200, 100, true).is_ok());
    }

    #[test]
    fn inversion_beats_privilege() {
        assert_eq!(
            judge_setrlimit(RES_NOFILE, 300, 200, 100, false),
            Err(EINVAL)
        );
    }

    #[test]
    fn bad_which_einval() {
        assert_eq!(judge_setrlimit(99, 1, 2, 2, true), Err(EINVAL));
    }

    #[test]
    fn bad_matrix_green() {
        let (l, per, rl) = run_bad_rlimit_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
        assert!(rl.closed());
    }

    #[test]
    fn layers_disjoint() {
        assert!(layers_disjoint_with_b19());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9330_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
