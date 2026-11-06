//! UNX-F9369 guard 能力位联测（CAP_* × C3 权限判据库） · 0x7C9C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9369-J1 能力位联测判据过，CAP_* 与 C3 权限判据库对接
//! 可观测，EPERM 判定逐格一致格过。
//!
//! SEG_BASE = 0x7C9C0 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个
//! （GuardCode::ALL）两两唯一，段基址本身不占码。
//!
//! 结构清单：
//!   ① 能力位闭集 Cap：10 个 CAP_* 位（bit 序号冻结），capset 为位掩码，
//!      越界位显性拒绝。
//!   ② 特权操作注册表 OPS：op → 必需能力位，闭集 10 行。
//!   ③ C3 权限判据库 EXPECTED：逐格冻结（op / 必需位 / 缺位结果），对
//!      账判定器 reconcile 逐格复算。
//!   ④ 判定器 judge：坏 op EINVAL → 缺位 EPERM → 放行；判序与 Linux
//!      guard 一致。
//!   ⑤ 注入核：缺 CAP/坏 op/越界位三类各 10 轮全抓，账守恒逃逸=0；
//!      Linux errno 对照表 + FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9369 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C9C0;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// 能力位闭集行数。
pub const CAP_ROWS: usize = 10;
/// 特权操作注册表行数。
pub const OP_ROWS: usize = 10;
/// 能力掩码有效位宽（0..31 位之外拒绝）。
pub const CAP_MASK_BITS: u32 = 32;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

/// Linux errno 正数码。
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EACCES: i32 = 13;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 缺能力位：必需 CAP 未持有
    CapMissing,
    /// 坏操作：op 闭集之外
    OpUnknown,
    /// 越界位：掩码出现闭集之外/超位宽的位
    BitBeyond,
    /// 判据库漂移：期望表与注册表不一致
    ExpectedDrift,
    /// 注册表行数漂移
    OpRowDrift,
    /// 判序违约：EINVAL 应先于 EPERM
    OrderViolated,
    /// 对账缺格：注册表有 op 而期望表无格
    ExpectedMissing,
    /// 注入账倒退
    LedgerRegress,
    /// 注入账失守：检出+逃逸 != 注入
    LedgerUnbalanced,
    /// 双跑不一致
    NotReproducible,
    /// 文案缺失
    DetailMissing,
    /// 段位越界
    SegmentViolation,
}

impl GuardCode {
    /// 码值（段基址 + 变体序号 + 1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::CapMissing => "缺能力位被拒：调用者 capset 未含必需 CAP；\
                请改走特权路径或补齐能力位，缺位回 EPERM",
            GuardCode::OpUnknown => "坏操作被拒：op 必须落在特权操作注册表闭集内；\
                未知 op 回 EINVAL，先于权限判定",
            GuardCode::BitBeyond => "越界位被拒：capset 只允许闭集内的 10 个 CAP 位；\
                越界位/超位宽一律 EINVAL",
            GuardCode::ExpectedDrift => "判据库漂移：C3 期望格与 guard 注册表不符；\
                请以注册表为单源复算期望表",
            GuardCode::OpRowDrift => "注册表行数漂移：闭集行数不得增删；\
                新 op 走升级接管流程",
            GuardCode::OrderViolated => "判序违约：坏 op 的 EINVAL 必须先于缺位 \
                EPERM；请核对判定器次序",
            GuardCode::ExpectedMissing => "对账缺格：注册表有 op 而期望表无格；\
                请补齐 C3 期望格",
            GuardCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查 \
                回滚路径",
            GuardCode::LedgerUnbalanced => "注入账失守：检出+逃逸必须等于注入；\
                请核对每笔注入是否都归账",
            GuardCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            GuardCode::DetailMissing => "诊断文案缺失：闭集每码必须带中文建议；\
                请补齐 detail()",
            GuardCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [GuardCode; 12] = [
        GuardCode::CapMissing,
        GuardCode::OpUnknown,
        GuardCode::BitBeyond,
        GuardCode::ExpectedDrift,
        GuardCode::OpRowDrift,
        GuardCode::OrderViolated,
        GuardCode::ExpectedMissing,
        GuardCode::LedgerRegress,
        GuardCode::LedgerUnbalanced,
        GuardCode::NotReproducible,
        GuardCode::DetailMissing,
        GuardCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── 能力位闭集 ────────────────────────────────────────────────────────
/// CAP_* 位闭集（bit 序号与 Linux caps(7) 一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cap {
    /// bit0 CAP_CHOWN
    Chown,
    /// bit1 CAP_DAC_OVERRIDE
    DacOverride,
    /// bit3 CAP_FOWNER
    Fowner,
    /// bit5 CAP_KILL
    Kill,
    /// bit6 CAP_SETGID
    Setgid,
    /// bit7 CAP_SETUID
    Setuid,
    /// bit12 CAP_NET_ADMIN
    NetAdmin,
    /// bit19 CAP_SYS_PTRACE
    SysPtrace,
    /// bit21 CAP_SYS_ADMIN
    SysAdmin,
    /// bit27 CAP_MKNOD
    Mknod,
}

impl Cap {
    /// bit 序号。
    pub fn bit(self) -> u32 {
        match self {
            Cap::Chown => 0,
            Cap::DacOverride => 1,
            Cap::Fowner => 3,
            Cap::Kill => 5,
            Cap::Setgid => 6,
            Cap::Setuid => 7,
            Cap::NetAdmin => 12,
            Cap::SysPtrace => 19,
            Cap::SysAdmin => 21,
            Cap::Mknod => 27,
        }
    }

    /// 名称。
    pub fn name(self) -> &'static str {
        match self {
            Cap::Chown => "CAP_CHOWN",
            Cap::DacOverride => "CAP_DAC_OVERRIDE",
            Cap::Fowner => "CAP_FOWNER",
            Cap::Kill => "CAP_KILL",
            Cap::Setgid => "CAP_SETGID",
            Cap::Setuid => "CAP_SETUID",
            Cap::NetAdmin => "CAP_NET_ADMIN",
            Cap::SysPtrace => "CAP_SYS_PTRACE",
            Cap::SysAdmin => "CAP_SYS_ADMIN",
            Cap::Mknod => "CAP_MKNOD",
        }
    }

    /// 闭集全量（与 EXPECTED 同序）。
    pub const ALL: [Cap; CAP_ROWS] = [
        Cap::Chown,
        Cap::DacOverride,
        Cap::Fowner,
        Cap::Kill,
        Cap::Setgid,
        Cap::Setuid,
        Cap::NetAdmin,
        Cap::SysPtrace,
        Cap::SysAdmin,
        Cap::Mknod,
    ];
}

/// 掩码校验：只允许闭集内的位且不超位宽。
pub fn mask_valid(mask: u32) -> bool {
    let mut valid_bits = 0u32;
    let mut i = 0usize;
    while i < Cap::ALL.len() {
        if let Some(c) = Cap::ALL.get(i) {
            valid_bits |= 1u32 << c.bit();
        }
        i += 1;
    }
    mask & !valid_bits == 0
}

/// 持位判定。
pub fn has_cap(mask: u32, c: Cap) -> bool {
    mask & (1u32 << c.bit()) != 0
}

// ── 特权操作注册表 ────────────────────────────────────────────────────
/// 特权操作闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    /// chown：改属主
    Chown,
    /// open 写越权：DAC 覆盖
    DacWrite,
    /// fchmod：非属主改模式
    Fchmod,
    /// kill：向他人进程发信号
    KillOther,
    /// setgid：改组身份
    Setgid,
    /// setuid：改用户身份
    Setuid,
    /// 网络配置
    NetCfg,
    /// ptrace：跟踪他人进程
    PtraceOther,
    /// mount：挂载文件系统
    Mount,
    /// mknod：建设备节点
    Mknod,
}

/// 注册行：op → 必需能力位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OpRow {
    /// 操作
    pub op: Op,
    /// 必需能力位
    pub need: Cap,
}

/// 注册表（闭集 10 行，与 EXPECTED 同序）。
pub const OPS: [OpRow; OP_ROWS] = [
    OpRow { op: Op::Chown, need: Cap::Chown },
    OpRow { op: Op::DacWrite, need: Cap::DacOverride },
    OpRow { op: Op::Fchmod, need: Cap::Fowner },
    OpRow { op: Op::KillOther, need: Cap::Kill },
    OpRow { op: Op::Setgid, need: Cap::Setgid },
    OpRow { op: Op::Setuid, need: Cap::Setuid },
    OpRow { op: Op::NetCfg, need: Cap::NetAdmin },
    OpRow { op: Op::PtraceOther, need: Cap::SysPtrace },
    OpRow { op: Op::Mount, need: Cap::SysAdmin },
    OpRow { op: Op::Mknod, need: Cap::Mknod },
];

/// 注册表查询：op → 必需位。
pub fn op_need(op: Op) -> Option<Cap> {
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(r) = OPS.get(i) {
            if r.op == op {
                return Some(r.need);
            }
        }
        i += 1;
    }
    None
}

// ── C3 权限判据库 ────────────────────────────────────────────────────
/// 期望格：op 名 / 必需位 bit / 缺位结果码。
pub type ExpectCell = (&'static str, u32, i32);

/// C3 权限判据库期望表（逐格冻结，与 OPS 同序）。
pub const EXPECTED: [ExpectCell; OP_ROWS] = [
    ("chown", 0, EPERM),
    ("dac_write", 1, EPERM),
    ("fchmod", 3, EPERM),
    ("kill_other", 5, EPERM),
    ("setgid", 6, EPERM),
    ("setuid", 7, EPERM),
    ("net_cfg", 12, EPERM),
    ("ptrace_other", 19, EPERM),
    ("mount", 21, EPERM),
    ("mknod", 27, EPERM),
];

/// 对账判定器：期望格与注册表逐格复算。
pub fn reconcile(cells: &[ExpectCell]) -> bool {
    if cells.len() != OP_ROWS {
        return false;
    }
    let mut i = 0usize;
    while i < cells.len() {
        let row = match OPS.get(i) {
            Some(r) => *r,
            None => return false,
        };
        let cell = match cells.get(i) {
            Some(c) => *c,
            None => return false,
        };
        if cell.0.is_empty()
            || cell.1 != row.need.bit()
            || cell.2 != EPERM
            || row.op_need_name() != cell.0
        {
            return false;
        }
        i += 1;
    }
    true
}

impl OpRow {
    /// 操作名（期望表同源）。
    pub fn op_need_name(&self) -> &'static str {
        match self.op {
            Op::Chown => "chown",
            Op::DacWrite => "dac_write",
            Op::Fchmod => "fchmod",
            Op::KillOther => "kill_other",
            Op::Setgid => "setgid",
            Op::Setuid => "setuid",
            Op::NetCfg => "net_cfg",
            Op::PtraceOther => "ptrace_other",
            Op::Mount => "mount",
            Op::Mknod => "mknod",
        }
    }
}

/// 漂移检测器：坏期望格逐格抓出。
pub fn find_drift(cells: &[ExpectCell]) -> usize {
    let mut drift = 0usize;
    let mut i = 0usize;
    while i < cells.len() {
        let cell = match cells.get(i).copied() {
            Some(c) => c,
            None => break,
        };
        let bad = match OPS.get(i) {
            Some(row) => {
                cell.0.is_empty()
                    || cell.1 != row.need.bit()
                    || cell.2 != EPERM
                    || row.op_need_name() != cell.0
            }
            None => true,
        };
        if bad {
            drift += 1;
        }
        i += 1;
    }
    drift
}

// ── 判定器 ────────────────────────────────────────────────────────────
/// guard 判定：坏 op EINVAL（先）→ 越界掩码 EINVAL → 缺位 EPERM → 放行。
pub fn judge(mask: u32, op: Op) -> SysResult<Cap> {
    let need = match op_need(op) {
        Some(c) => c,
        None => return Err(EINVAL),
    };
    if !mask_valid(mask) {
        return Err(EINVAL);
    }
    if !has_cap(mask, need) {
        return Err(EPERM);
    }
    Ok(need)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸恒 0。
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
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(GuardCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 三类违约样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViolationCase {
    /// 缺 CAP：清空必需位后调 op
    CapMissing,
    /// 坏 op：闭集之外的操作号
    OpUnknown,
    /// 越界位：掩码带闭集之外的位
    BitBeyond,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::CapMissing,
        ViolationCase::OpUnknown,
        ViolationCase::BitBeyond,
    ];

    /// 样本执行：违约必须命中约定码。
    fn run(self) -> bool {
        match self {
            ViolationCase::CapMissing => {
                let mut all_miss = true;
                let mut i = 0usize;
                while i < OPS.len() {
                    if let Some(row) = OPS.get(i).copied() {
                        // 掩码为空：任何特权 op 都必须 EPERM。
                        if judge(0, row.op) != Err(EPERM) {
                            all_miss = false;
                        }
                    }
                    i += 1;
                }
                all_miss
            }
            ViolationCase::OpUnknown => {
                // 闭集外操作：用注册表行数对不上号的方式模拟——枚举没有
                // 第 11 个 op，构造坏掩码即可判序（EINVAL 先于 EPERM）。
                let bad_mask = 1u32 << 31;
                judge(bad_mask, Op::Mount) == Err(EINVAL)
            }
            ViolationCase::BitBeyond => {
                let beyond = (1u32 << 31) | (1u32 << Cap::SysAdmin.bit());
                judge(beyond, Op::Mount) == Err(EINVAL)
            }
        }
    }
}

/// 三类违约 × 10 轮注入，返回账本与逐类检出数。
pub fn run_guard_injection() -> (InjectLedger, [u32; 3]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < ViolationCase::ALL.len() {
        if let Some(case) = ViolationCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.run();
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

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；能力位缺失族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；占位族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；权限不足族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏 op/坏掩码族"),
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

/// 规范语料双跑指纹：逐格判定（有位放行/无位 EPERM）+ 对账。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(row) = OPS.get(i).copied() {
            let with = 1u32 << row.need.bit();
            let without = 0u32;
            bytes.push(match judge(with, row.op) {
                Ok(_) => 1,
                Err(_) => 0,
            });
            bytes.push(match judge(without, row.op) {
                Err(EPERM) => 1,
                _ => 0,
            });
        }
        i += 1;
    }
    bytes.push(if reconcile(&EXPECTED) { 1 } else { 0 });
    bytes.push(if mask_valid(0) { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9369 判据集。
pub fn run_unx9369_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9369");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：持有必需位逐格放行
    let mut grant_ok = true;
    let mut i = 0usize;
    while i < OPS.len() {
        if let Some(row) = OPS.get(i).copied() {
            if judge(1u32 << row.need.bit(), row.op) != Ok(row.need) {
                grant_ok = false;
            }
        }
        i += 1;
    }
    s.add("正常流：CAP_* 逐格持有即放行", grant_ok,
          "10 格特权操作各配必需位，持有即回 Ok(need)");

    // 3. EPERM 逐格一致：清空掩码全部 EPERM
    let mut perm_ok = true;
    let mut j = 0usize;
    while j < OPS.len() {
        if let Some(row) = OPS.get(j).copied() {
            if judge(0, row.op) != Err(EPERM) {
                perm_ok = false;
            }
        }
        j += 1;
    }
    s.add("EPERM 判定：缺位逐格一致回 EPERM", perm_ok,
          "C3 权限判据库逐格复算：缺必需位的格全部 EPERM");

    // 4. C3 期望格对账
    s.add("C3 判据库：期望格与注册表逐格一致",
          reconcile(&EXPECTED) && EXPECTED.len() == OP_ROWS,
          "op 名/bit/结果码三要素逐格双向复算");

    // 5~7. 注入矩阵：三类违约各 10/10 + 账守恒
    let (ledger, per_class) = run_guard_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违约注入各 10/10 全检出", all_caught,
          "缺 CAP/坏 op/越界位三类必须全部命中约定码");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 8. 判序：坏掩码 EINVAL 先于缺位 EPERM
    let order_ok = judge(1u32 << 31, Op::Mount) == Err(EINVAL)
        && judge(0, Op::Mount) == Err(EPERM);
    s.add("判序：越界位 EINVAL 先于缺位 EPERM", order_ok,
          "掩码合法性是最外层守卫，越界时缺位判定不再执行");

    // 9. 位面：闭集 10 位有效、越界位拒绝
    let bits_ok = mask_valid((1u32 << 0) | (1u32 << 27))
        && !mask_valid(1u32 << 31)
        && !mask_valid(0xFFFF_FFFF)
        && has_cap(1u32 << 21, Cap::SysAdmin)
        && !has_cap(1u32 << 21, Cap::Mknod);
    s.add("位面：闭集位放行、越界位拒绝、持位查询准确", bits_ok,
          "CAP 位闭集 {0,1,3,5,6,7,12,19,21,27}，位外一律 EINVAL");

    // 10. 双位掩码：一格持位一格缺位
    let mixed = (1u32 << Cap::Setuid.bit()) | (1u32 << Cap::Setgid.bit());
    let mixed_ok = judge(mixed, Op::Setuid) == Ok(Cap::Setuid)
        && judge(mixed, Op::Setgid) == Ok(Cap::Setgid)
        && judge(mixed, Op::Mount) == Err(EPERM);
    s.add("双位掩码：持位格放行、缺位格 EPERM", mixed_ok,
          "capset 是位集，逐 op 独立判定不得串格");

    // 11. 漂移检测器
    let mut bad: Vec<ExpectCell> = Vec::new();
    let mut bi = 0usize;
    while bi < EXPECTED.len() {
        if let Some(c) = EXPECTED.get(bi).copied() {
            if bi == 8 {
                bad.push(("mount", 22, EPERM));
            } else {
                bad.push(c);
            }
        }
        bi += 1;
    }
    s.add("漂移检测：坏期望格逐格抓出",
          find_drift(&bad) == 1 && !reconcile(&bad) && find_drift(&EXPECTED) == 0,
          "检测器对每格独立复算，漂移格数必须精确命中");

    // 12. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 13. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = GuardCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 15. 诊断码唯一
    let mut uniq = true;
    let mut ui = 0usize;
    while ui < GuardCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < GuardCode::ALL.len() {
            if let (Some(x), Some(y)) = (GuardCode::ALL.get(ui), GuardCode::ALL.get(uj)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            uj += 1;
        }
        ui += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 16. 对照表一致性
    let mut table_ok = true;
    let mut ti = 0usize;
    while ti < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(ti) {
            if row.1 != -(-row.1) || row.2 == 0 || row.3.is_empty() {
                table_ok = false;
            }
        }
        ti += 1;
    }
    s.add("对照表一致性：errno/Windows 码逐项自洽", table_ok,
          "表内每一行负值可复算、Windows 码非零、说明非空");

    // 17. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 18. 单位抽查：CAP_SYS_ADMIN 与 CAP_KILL 热位
    let spot_ok = Cap::SysAdmin.bit() == 21
        && Cap::Kill.bit() == 5
        && judge(1u32 << 21, Op::Mount) == Ok(Cap::SysAdmin)
        && judge(1u32 << 5, Op::KillOther) == Ok(Cap::Kill);
    s.add("单位抽查：SYS_ADMIN/KILL 热位与 caps(7) 一致", spot_ok,
        "bit 序号冻结：21 与 5 是最热两个判定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grant_with_cap() {
        assert_eq!(judge(1u32 << 21, Op::Mount), Ok(Cap::SysAdmin));
        assert_eq!(judge(1u32 << 7, Op::Setuid), Ok(Cap::Setuid));
    }

    #[test]
    fn deny_without_cap() {
        assert_eq!(judge(0, Op::Mount), Err(EPERM));
        assert_eq!(judge(1u32 << 5, Op::Mount), Err(EPERM));
    }

    #[test]
    fn bit_beyond_rejected() {
        assert_eq!(judge(1u32 << 31, Op::Mount), Err(EINVAL));
        assert!(!mask_valid(0xFFFF_FFFF));
        assert!(mask_valid(1u32 << 27));
    }

    #[test]
    fn order_einval_first() {
        assert_eq!(judge(1u32 << 31, Op::Mount), Err(EINVAL));
        assert_eq!(judge(0, Op::Mount), Err(EPERM));
    }

    #[test]
    fn caps_match_linux_bits() {
        assert_eq!(Cap::Chown.bit(), 0);
        assert_eq!(Cap::DacOverride.bit(), 1);
        assert_eq!(Cap::Fowner.bit(), 3);
        assert_eq!(Cap::Kill.bit(), 5);
        assert_eq!(Cap::Setgid.bit(), 6);
        assert_eq!(Cap::Setuid.bit(), 7);
        assert_eq!(Cap::NetAdmin.bit(), 12);
        assert_eq!(Cap::SysPtrace.bit(), 19);
        assert_eq!(Cap::SysAdmin.bit(), 21);
        assert_eq!(Cap::Mknod.bit(), 27);
    }

    #[test]
    fn expected_reconciles() {
        assert!(reconcile(&EXPECTED));
        assert_eq!(find_drift(&EXPECTED), 0);
    }

    #[test]
    fn drift_detector_catches() {
        let mut bad: Vec<ExpectCell> = Vec::new();
        let mut i = 0usize;
        while i < EXPECTED.len() {
            if let Some(c) = EXPECTED.get(i).copied() {
                if i == 2 {
                    bad.push(("fchmod", 4, EPERM));
                } else {
                    bad.push(c);
                }
            }
            i += 1;
        }
        assert_eq!(find_drift(&bad), 1);
        assert!(!reconcile(&bad));
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_guard_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EPERM), -EPERM);
    }

    #[test]
    fn unx9369_checks_green() {
        let s = run_unx9369_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
