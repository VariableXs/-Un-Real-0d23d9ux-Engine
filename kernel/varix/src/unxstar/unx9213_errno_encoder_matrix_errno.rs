//! UNX-F9213 错误码编码器一致性矩阵（Errno→sysret 逐码） · 0x7A2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9213 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9213-J1 Errno 全集逐码注入
//!   判据过，sysret 编码逐码一致格过，与 B13 映射账零漂移格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 编码面是「双通道」：x86-64 用 rax 承载返回值、CF 承载成败。错误时
//!      rax = -errno（64 位二补数）、carry=1；成功时 rax = 值、carry=0。
//!      本卡把两条通道拆成 Sysret { rax, carry } 逐字段比对，避免只比 rax。
//!   ② 合法错误窗口是 1..=4095（MAX_ERRNO）：0 不是错误码（成功），4096 起
//!      会被 IS_ERR_VALUE 漏判成「指针」，所以编码器必须在入口拒掉，而不是
//!      让它流进号段。
//!   ③ 成功值也有禁区：任何落在 [2^64-4095, 2^64-1] 的成功返回值都会被用户态
//!      误读成错误。encode_ok 必须显性拒绝该窗口，这是「双向一致」的另一半。
//!   ④ 32 位消费核只看 eax：rax 必须等于 -errno 的 32 位符号扩展，否则 glibc
//!      的 `if (rax < 0)` 分支会错。逐码断言 `rax as i32 == -code`。
//!   ⑤ 「与 B13 映射账零漂移」用冻结账复算：本模块的 errno 表与 B13 账逐行
//!      比名、比码、比行数，任何一行改动都会被对账格抓出来。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9213 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A2C0;
/// Linux 错误窗口上界（IS_ERR_VALUE 的 4095）。
pub const MAX_ERRNO: u32 = 4095;
/// 错误窗口起点（64 位视角）。
pub const ERR_WINDOW_START: u64 = u64::MAX - MAX_ERRNO as u64 + 1;
/// 单类注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 参数非法（编码器拒绝）。
pub const EINVAL: u32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 编码器诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncCode {
    CarryMissing,
    CarryLeak,
    SignExtendBad,
    ZeroRejected,
    OversizeRejected,
    BoundaryOff,
    OkWindowClash,
    LedgerDrift,
    DuplicateCode,
    StageSkip,
    NotReproducible,
    SegmentViolation,
}

impl EncCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EncCode::CarryMissing => "错误返回缺 CF：错误码必须置 carry=1，否则用户态当成正常返回值",
            EncCode::CarryLeak => "成功返回带 CF：成功路径必须清 carry，否则值与错误窗口混淆",
            EncCode::SignExtendBad => "符号扩展错：rax 必须等于 -errno 的 32→64 位符号扩展，glibc 只判 eax<0",
            EncCode::ZeroRejected => "零码被拒：errno==0 表示成功，编码器入口必须显性拒绝",
            EncCode::OversizeRejected => "超限被拒：errno>4095 会掉出 IS_ERR_VALUE 窗口，必须在入口拦下",
            EncCode::BoundaryOff => "边界错：4095 必须通过、4096 必须拒绝，两侧判据不可对调",
            EncCode::OkWindowClash => "成功值落进错误窗口：2^64-4095 起的返回值会被误读成错误，encode_ok 必须拒绝",
            EncCode::LedgerDrift => "与 B13 映射账漂移：名/码/行数任一处不一致即红，请按冻结账复算",
            EncCode::DuplicateCode => "码值重复：两个 errno 共用一个数码会让解码不可逆",
            EncCode::StageSkip => "编码阶段跳级：Classify→Encode→Verify→Emit 只许前进",
            EncCode::NotReproducible => "双跑不一致：同码两次编码不同；请排查隐藏状态",
            EncCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [EncCode; 12] = [
        EncCode::CarryMissing, EncCode::CarryLeak, EncCode::SignExtendBad,
        EncCode::ZeroRejected, EncCode::OversizeRejected, EncCode::BoundaryOff,
        EncCode::OkWindowClash, EncCode::LedgerDrift, EncCode::DuplicateCode,
        EncCode::StageSkip, EncCode::NotReproducible, EncCode::SegmentViolation,
    ];
}

// ── errno 表（Linux x86-64 取值） ─────────────────────────────────────
/// 一行 errno：(名字, 正数码, Windows 近似码)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrnoRow {
    pub name: &'static str,
    pub code: u32,
    pub win: u32,
}

/// 本模块 errno 表（32 行，覆盖文件/内存/进程/网络四族）。
pub const ERRNO_TABLE: [ErrnoRow; 32] = [
    ErrnoRow { name: "EPERM", code: 1, win: 1314 },
    ErrnoRow { name: "ENOENT", code: 2, win: 2 },
    ErrnoRow { name: "ESRCH", code: 3, win: 1168 },
    ErrnoRow { name: "EINTR", code: 4, win: 995 },
    ErrnoRow { name: "EIO", code: 5, win: 1117 },
    ErrnoRow { name: "ENXIO", code: 6, win: 2 },
    ErrnoRow { name: "E2BIG", code: 7, win: 206 },
    ErrnoRow { name: "ENOEXEC", code: 8, win: 193 },
    ErrnoRow { name: "EBADF", code: 9, win: 6 },
    ErrnoRow { name: "ECHILD", code: 10, win: 128 },
    ErrnoRow { name: "EAGAIN", code: 11, win: 232 },
    ErrnoRow { name: "ENOMEM", code: 12, win: 8 },
    ErrnoRow { name: "EACCES", code: 13, win: 5 },
    ErrnoRow { name: "EFAULT", code: 14, win: 998 },
    ErrnoRow { name: "ENOTBLK", code: 15, win: 1 },
    ErrnoRow { name: "EBUSY", code: 16, win: 32 },
    ErrnoRow { name: "EEXIST", code: 17, win: 183 },
    ErrnoRow { name: "EXDEV", code: 18, win: 17 },
    ErrnoRow { name: "ENODEV", code: 19, win: 15 },
    ErrnoRow { name: "ENOTDIR", code: 20, win: 267 },
    ErrnoRow { name: "EISDIR", code: 21, win: 267 },
    ErrnoRow { name: "EINVAL", code: 22, win: 87 },
    ErrnoRow { name: "ENFILE", code: 23, win: 4 },
    ErrnoRow { name: "EMFILE", code: 24, win: 4 },
    ErrnoRow { name: "ENOTTY", code: 25, win: 1 },
    ErrnoRow { name: "EFBIG", code: 27, win: 223 },
    ErrnoRow { name: "ENOSPC", code: 28, win: 112 },
    ErrnoRow { name: "ESPIPE", code: 29, win: 29 },
    ErrnoRow { name: "EROFS", code: 30, win: 19 },
    ErrnoRow { name: "EPIPE", code: 32, win: 109 },
    ErrnoRow { name: "ERANGE", code: 34, win: 534 },
    ErrnoRow { name: "ENOSYS", code: 38, win: 1 },
];

/// B13 映射账（冻结副本；对账格的唯一依据）。
pub const B13_LEDGER: [(&str, u32); 32] = [
    ("EPERM", 1), ("ENOENT", 2), ("ESRCH", 3), ("EINTR", 4),
    ("EIO", 5), ("ENXIO", 6), ("E2BIG", 7), ("ENOEXEC", 8),
    ("EBADF", 9), ("ECHILD", 10), ("EAGAIN", 11), ("ENOMEM", 12),
    ("EACCES", 13), ("EFAULT", 14), ("ENOTBLK", 15), ("EBUSY", 16),
    ("EEXIST", 17), ("EXDEV", 18), ("ENODEV", 19), ("ENOTDIR", 20),
    ("EISDIR", 21), ("EINVAL", 22), ("ENFILE", 23), ("EMFILE", 24),
    ("ENOTTY", 25), ("EFBIG", 27), ("ENOSPC", 28), ("ESPIPE", 29),
    ("EROFS", 30), ("EPIPE", 32), ("ERANGE", 34), ("ENOSYS", 38),
];

/// 查表：名字 → 行。
pub fn row_of(name: &str) -> Option<ErrnoRow> {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(r) = ERRNO_TABLE.get(i) {
            if r.name == name {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

/// 查表：数码 → 行。
pub fn row_of_code(code: u32) -> Option<ErrnoRow> {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(r) = ERRNO_TABLE.get(i) {
            if r.code == code {
                return Some(*r);
            }
        }
        i += 1;
    }
    None
}

// ── sysret 编码 ───────────────────────────────────────────────────────
/// 一次系统调用返回的两个通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sysret {
    /// 返回值通道（x86-64 的 rax）
    pub rax: u64,
    /// 错误标志（x86-64 的 CF = 1）
    pub carry: bool,
}

impl Sysret {
    /// 32 位消费核看到的 eax。
    pub fn eax(&self) -> i32 {
        self.rax as i32
    }
}

/// 解码结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decoded {
    /// 成功返回值
    Value(u64),
    /// 错误码
    Errno(u32),
    /// 通道自相矛盾（carry=1 但码不在 1..=4095）
    Malformed,
}

/// 该 64 位值是否落在错误窗口 [2^64-4095, 2^64-1]。
pub fn in_error_window(v: u64) -> bool {
    v >= ERR_WINDOW_START
}

/// 编码错误：errno 必须落在 1..=MAX_ERRNO。
pub fn encode_errno(errno: u32) -> Result<Sysret, u32> {
    if errno == 0 || errno > MAX_ERRNO {
        return Err(EINVAL);
    }
    let rax = (-(errno as i64)) as u64;
    Ok(Sysret { rax, carry: true })
}

/// 编码成功：值不得落进错误窗口。
pub fn encode_ok(value: u64) -> Result<Sysret, u32> {
    if in_error_window(value) {
        return Err(EINVAL);
    }
    Ok(Sysret { rax: value, carry: false })
}

/// 解码：carry 通道优先，rax 只在与 carry 自洽时才取用。
pub fn decode(ret: Sysret) -> Decoded {
    if !ret.carry {
        return Decoded::Value(ret.rax);
    }
    let code = 0u64.wrapping_sub(ret.rax);
    if code == 0 || code > MAX_ERRNO as u64 {
        return Decoded::Malformed;
    }
    Decoded::Errno(code as u32)
}

/// 逐码一致性：编码后 rax、carry、eax、解码四元组全部正确。
pub fn code_is_consistent(row: &ErrnoRow) -> bool {
    let enc = match encode_errno(row.code) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let signed_ok = enc.eax() == -(row.code as i32);
    let decode_ok = decode(enc) == Decoded::Errno(row.code);
    let ok_window = encode_ok(enc.rax) == Err(EINVAL);
    enc.carry && signed_ok && decode_ok && ok_window
}

// ── 编码阶段状态机 ────────────────────────────────────────────────────
/// 编码流水阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncStage {
    Classify,
    Encode,
    Verify,
    Emit,
}

impl EncStage {
    fn ordinal(self) -> u32 {
        match self {
            EncStage::Classify => 0,
            EncStage::Encode => 1,
            EncStage::Verify => 2,
            EncStage::Emit => 3,
        }
    }
}

/// 阶段推进：只许 +1。
pub fn advance(cur: EncStage, next: EncStage) -> Result<EncStage, u32> {
    if next.ordinal() == cur.ordinal() + 1 {
        Ok(next)
    } else {
        Err(EINVAL)
    }
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 三类编码违约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncInject {
    /// 错误码 0
    Zero,
    /// 错误码 4096
    Oversize,
    /// 成功值落进错误窗口
    OkClash,
}

impl EncInject {
    /// 对应诊断码。
    pub fn code(self) -> EncCode {
        match self {
            EncInject::Zero => EncCode::ZeroRejected,
            EncInject::Oversize => EncCode::OversizeRejected,
            EncInject::OkClash => EncCode::OkWindowClash,
        }
    }

    /// 闭集全量。
    pub const ALL: [EncInject; 3] = [EncInject::Zero, EncInject::Oversize, EncInject::OkClash];
}

/// 编码账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub attempts: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    pub const fn zero() -> Ledger {
        Ledger { attempts: 0, detected: 0, escaped: 0 }
    }

    pub fn record(&mut self, caught: bool) -> bool {
        self.attempts = self.attempts.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }

    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.attempts
    }
}

/// 三类注入各 INJECT_ROUNDS 轮：违约必须被编码器拒绝（EINVAL）。
pub fn run_injections() -> (Ledger, [u32; 3]) {
    let mut ledger = Ledger::zero();
    let mut per = [0u32; 3];
    let mut ci = 0usize;
    while ci < EncInject::ALL.len() {
        if let Some(case) = EncInject::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    EncInject::Zero => encode_errno(0) == Err(EINVAL),
                    EncInject::Oversize => {
                        encode_errno(MAX_ERRNO + 1) == Err(EINVAL)
                            && encode_errno(u32::MAX) == Err(EINVAL)
                    }
                    EncInject::OkClash => {
                        encode_ok(ERR_WINDOW_START) == Err(EINVAL)
                            && encode_ok(u64::MAX) == Err(EINVAL)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(s) = per.get_mut(ci) {
                        *s = s.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per)
}

/// 全表逐码矩阵：每码编码-解码-符号扩展-窗口互斥四格，返回不符码数。
pub fn matrix_mismatches() -> u32 {
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if !code_is_consistent(row) {
                bad += 1;
            }
        }
        i += 1;
    }
    bad
}

// ── 对账（B13 账 + Windows 近似） ─────────────────────────────────────
/// 与 B13 账的行数差。
pub fn ledger_len_drift() -> u32 {
    let a = ERRNO_TABLE.len() as u32;
    let b = B13_LEDGER.len() as u32;
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// 与 B13 账逐行比对，返回漂移行数。
pub fn ledger_drift() -> u32 {
    let mut drift = ledger_len_drift();
    let mut i = 0usize;
    while i < B13_LEDGER.len() {
        if let Some((name, code)) = B13_LEDGER.get(i).copied() {
            match row_of(name) {
                Some(row) if row.code == code => {}
                _ => drift = drift.saturating_add(1),
            }
        }
        i += 1;
    }
    drift
}

/// 每个 errno 都必须有 Windows 近似码（0 视为缺省）。
pub fn windows_complete() -> bool {
    ERRNO_TABLE.iter().all(|r| r.win > 0)
}

// ── 指纹 ──────────────────────────────────────────────────────────────
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

/// 规范语料：全表逐码的 (rax 低 32, carry, 解码码) 入指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if let Ok(ret) = encode_errno(row.code) {
                bytes.push((ret.rax & 0xFF) as u8);
                bytes.push(((ret.rax >> 32) & 0xFF) as u8);
                bytes.push(if ret.carry { 1 } else { 0 });
                bytes.push(row.code as u8);
            }
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9213 判据集（21 条静态 + 3 条动态）。
pub fn run_unx9213_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9213");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：EBADF 编码
    let ret = encode_errno(9);
    let eb_ok = ret == Ok(Sysret { rax: (-9i64) as u64, carry: true });
    s.add("正常流：EBADF → rax=2^64-9 / CF=1", eb_ok,
          "错误码固定编码为 -errno 的 64 位二补数");

    // 3. 全表逐码往返
    let roundtrip = ERRNO_TABLE.iter().all(|r| match encode_errno(r.code) {
        Ok(ret) => decode(ret) == Decoded::Errno(r.code) && ret.carry,
        Err(_) => false,
    });
    s.add("逐码往返：32 码编码后解码回到原码", roundtrip,
          "解码器必须把每个错误码原样还原，不许混淆相邻码");

    // 4. 32 位符号扩展
    let sign32 = ERRNO_TABLE.iter().all(|r| match encode_errno(r.code) {
        Ok(ret) => ret.eax() == -(r.code as i32),
        Err(_) => false,
    });
    s.add("32 位一致性：rax as i32 == -code", sign32,
          "glibc 只判 eax<0，符号扩展错会导致误判成功");

    // 5. 零码拒绝
    s.add("边界：errno=0 被拒（EINVAL）", encode_errno(0) == Err(EINVAL),
          "0 表示成功，不得混入错误窗口");

    // 6. 超限拒绝
    s.add("边界：4096 / 5000 / u32::MAX 全拒",
          encode_errno(MAX_ERRNO + 1) == Err(EINVAL)
              && encode_errno(5000) == Err(EINVAL)
              && encode_errno(u32::MAX) == Err(EINVAL),
          "超限码会掉出 IS_ERR_VALUE 窗口，必须在入口拦下");

    // 7. MAX_ERRNO 边界两侧
    let edge_ok = encode_errno(MAX_ERRNO).is_ok() && encode_errno(MAX_ERRNO + 1).is_err();
    let edge_val = match encode_errno(MAX_ERRNO) {
        Ok(ret) => ret.rax == ERR_WINDOW_START,
        Err(_) => false,
    };
    s.add("边界：4095 通过且恰触窗口起点，4096 拒", edge_ok && edge_val,
          "窗口起点与最大合法码必须精确咬合");

    // 8. 成功编码
    let ok1 = encode_ok(0) == Ok(Sysret { rax: 0, carry: false });
    let ok2 = encode_ok(4096) == Ok(Sysret { rax: 4096, carry: false });
    s.add("成功：rax=值且 CF=0", ok1 && ok2,
          "成功路径清 CF，值原样透传");

    // 9. 成功值落错误窗口
    s.add("成功值禁区：窗口起点与 u64::MAX 被拒",
          encode_ok(ERR_WINDOW_START) == Err(EINVAL)
              && encode_ok(u64::MAX) == Err(EINVAL)
              && encode_ok(ERR_WINDOW_START - 1).is_ok(),
          "窗口下沿前一个值仍合法，边界不可差一");

    // 10. 双向互斥：错误码编码值不能当成功值再编码
    let exclusive = ERRNO_TABLE.iter().all(|r| match encode_errno(r.code) {
        Ok(ret) => encode_ok(ret.rax).is_err(),
        Err(_) => false,
    });
    s.add("双向互斥：错误编码值不得再作成功值", exclusive,
          "两个方向共用一条数值轴，窗口必须互斥");

    // 11. 解码自洽
    let dec_ok = decode(Sysret { rax: 5, carry: false }) == Decoded::Value(5)
        && decode(Sysret { rax: (-9i64) as u64, carry: true }) == Decoded::Errno(9)
        && decode(Sysret { rax: 0, carry: true }) == Decoded::Malformed
        && decode(Sysret { rax: (-5000i64) as u64, carry: true }) == Decoded::Malformed;
    s.add("解码：carry 与码窗口自洽，否则 Malformed", dec_ok,
          "carry=1 但码越窗说明通道矛盾，必须显性标记");

    // 12. 与 B13 账零漂移
    let drift = ledger_drift();
    s.add("对账：与 B13 映射账零漂移（32 行逐行同名同码）",
          drift == 0 && ledger_len_drift() == 0,
          "任何一行改名/改码都会被对账格抓出");

    // 13. 表内码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        let mut j = i + 1;
        while j < ERRNO_TABLE.len() {
            if let (Some(a), Some(b)) = (ERRNO_TABLE.get(i), ERRNO_TABLE.get(j)) {
                if a.code == b.code {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("表内唯一：32 个 errno 数码两两不同", uniq,
          "码值重复会让解码不可逆");

    // 14. 矩阵逐格
    let mism = matrix_mismatches();
    s.add("矩阵：全表逐码四格（rax/carry/eax/往返）零不符", mism == 0,
          "四格是编码器的最小完备断言集");

    // 15. 注入 3 类 10/10
    let (ledger, per) = run_injections();
    s.add("注入：三类编码违约各 10/10 全检出",
          per.iter().all(|n| *n == INJECT_ROUNDS),
          "零码/超限/成功窗口冲突必须逐类全抓");

    // 16. 守恒
    s.add("守恒：尝试=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.attempts == EncInject::ALL.len() as u32 * INJECT_ROUNDS,
          "30 次注入必须 30 次检出");

    // 17. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：编码指纹相等", f1 == f2,
          "同码两次编码必须逐字节相同");

    // 18. 状态机
    let sm_ok = advance(EncStage::Classify, EncStage::Encode) == Ok(EncStage::Encode)
        && advance(EncStage::Encode, EncStage::Verify) == Ok(EncStage::Verify)
        && advance(EncStage::Verify, EncStage::Emit) == Ok(EncStage::Emit)
        && advance(EncStage::Classify, EncStage::Emit) == Err(EINVAL)
        && advance(EncStage::Emit, EncStage::Classify) == Err(EINVAL);
    s.add("状态机：四阶段只进不退，跳级被拒", sm_ok,
          "先分类、再编码、再校验、最后落返回寄存器");

    // 19. Windows 近似码齐全
    s.add("对账：每码都有 Windows 近似码",
          windows_complete() && row_of("EBADF").map(|r| r.win) == Some(6),
          "近似码只作说明，不参与判定，但不得缺行");

    // 20. 文案闭集
    s.add("文案闭集：12 个诊断码均带中文建议",
          EncCode::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");

    // 21. 收口
    let before = s.len();
    let trunc = s.truncated();
    s.add("收口：判据不少于 20、未截断、账行数冻结", before >= 20 && !trunc
              && ERRNO_TABLE.len() == 32 && B13_LEDGER.len() == 32,
          "表行数只增不减；动态明细槽仅用 3 个");

    // 动态明细（≤8）
    let enc_text = format!("EBADF rax=0x{:016X} CF={}", (-9i64) as u64, true);
    s.add_dyn("编码快照", eb_ok, &enc_text);
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.attempts, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("编码指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_ebadf_exactly() {
        let ret = encode_errno(9).unwrap_or(Sysret { rax: 0, carry: false });
        assert!(ret.carry);
        assert_eq!(ret.rax, u64::MAX - 8);
        assert_eq!(ret.eax(), -9);
    }

    #[test]
    fn every_row_roundtrips() {
        for r in ERRNO_TABLE.iter() {
            let ret = encode_errno(r.code).unwrap_or(Sysret { rax: 0, carry: false });
            assert_eq!(decode(ret), Decoded::Errno(r.code));
            assert_eq!(ret.eax(), -(r.code as i32));
        }
    }

    #[test]
    fn zero_and_oversize_are_rejected() {
        assert_eq!(encode_errno(0), Err(EINVAL));
        assert_eq!(encode_errno(4096), Err(EINVAL));
        assert_eq!(encode_errno(u32::MAX), Err(EINVAL));
    }

    #[test]
    fn max_errno_boundary() {
        assert!(encode_errno(4095).is_ok());
        assert_eq!(encode_errno(4095).unwrap_or(Sysret { rax: 0, carry: false }).rax,
                   ERR_WINDOW_START);
        assert!(encode_errno(4096).is_err());
    }

    #[test]
    fn success_window_is_exclusive() {
        assert!(encode_ok(0).is_ok());
        assert!(encode_ok(ERR_WINDOW_START - 1).is_ok());
        assert_eq!(encode_ok(ERR_WINDOW_START), Err(EINVAL));
        assert_eq!(encode_ok(u64::MAX), Err(EINVAL));
    }

    #[test]
    fn decode_rejects_inconsistent_carry() {
        assert_eq!(decode(Sysret { rax: 0, carry: true }), Decoded::Malformed);
        assert_eq!(decode(Sysret { rax: 0, carry: false }), Decoded::Value(0));
        assert_eq!(decode(Sysret { rax: (-4096i64) as u64, carry: true }), Decoded::Malformed);
    }

    #[test]
    fn ledger_has_no_drift() {
        assert_eq!(ledger_drift(), 0);
        assert_eq!(ledger_len_drift(), 0);
        assert_eq!(B13_LEDGER.len(), ERRNO_TABLE.len());
    }

    #[test]
    fn matrix_is_clean() {
        assert_eq!(matrix_mismatches(), 0);
    }

    #[test]
    fn injections_are_all_caught() {
        let (l, per) = run_injections();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.attempts, 30);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn stage_machine_is_forward_only() {
        assert_eq!(advance(EncStage::Classify, EncStage::Encode), Ok(EncStage::Encode));
        assert_eq!(advance(EncStage::Encode, EncStage::Emit), Err(EINVAL));
        assert_eq!(advance(EncStage::Emit, EncStage::Verify), Err(EINVAL));
    }

    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9213_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(s.len() >= 20);
    }
}
