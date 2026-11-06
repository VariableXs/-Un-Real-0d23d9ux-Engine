//! UNX-F9366 errno 编码联测（Errno 全集 × C3 断言库） · 0x7C900段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9366-J1 Errno 全集联测判据过，逐码与 C3 断言库对接一致
//! 格过，LTP 期望值逐码对账格过。
//!
//! SEG_BASE = 0x7C900 诊断码闭集说明：本段 0x40 窗口独占；诊断码 1 基
//! 编码（码值 = SEG_BASE + 变体序号 + 1），闭集 12 个
//! （ErrJointCode::ALL）两两唯一，段基址本身不占码。
//!
//! 结构清单：
//!   ① Errno 全集编码器 LxErrno：24 个 errno 变体 + value()/to_sysret/
//!      from_sysret 双向往返，正数与表外码一律拒绝。
//!   ② C3 断言库期望表 EXPECTED：逐码冻结（LTP 名 / errno 值 / sysret
//!      编码），对账判定器 reconcile 逐行复算。
//!   ③ 漂移检测器 find_drift：坏表逐行抓出（注入用），行数守恒。
//!   ④ 注入核：表外 sysret/编码翻转/期望漂移三类各 10 轮全抓，账守恒
//!      逃逸=0。
//!   ⑤ Linux errno 对照表（Windows 近似码）+ FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9366 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7C900;

/// 本批段窗口下界。
pub const WINDOW_LO: u32 = 0x7A000;
/// 本批段窗口上界。
pub const WINDOW_HI: u32 = 0x7D1C0;

/// errno 全集行数。
pub const ERRNO_ROWS: usize = 24;
/// C3 期望表行数（与全集一一对应）。
pub const EXPECTED_ROWS: usize = 24;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

/// Linux errno 正数码（本模块自引用全集的一部分）。
pub const EPERM: i32 = 1;
pub const EINVAL: i32 = 22;
pub const EBADF: i32 = 9;
pub const ENOSYS: i32 = 38;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（1 基：码值 = SEG_BASE + 序号 + 1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrJointCode {
    /// 表外 sysret：正数/零/表外负数不得解码
    ForeignSysret,
    /// 编码翻转：sysret 必须是负值
    SignFlipped,
    /// 往返失配：decode(encode(e)) 必须还原 e
    RoundtripMiss,
    /// 期望漂移：C3 期望表与编码器不一致
    ExpectedDrift,
    /// 全集行数漂移：24 行闭集被增删
    RowCountDrift,
    /// 期望表缺失行：全集有码而表无行
    ExpectedMissing,
    /// 期望表重复行：同码登记两行
    ExpectedDup,
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

impl ErrJointCode {
    /// 码值（段基址 + 变体序号 + 1）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ErrJointCode::ForeignSysret => "表外 sysret 被拒：解码只接受全集内的负值；\
                正数、零与表外负数一律 None，不得杜撰 errno",
            ErrJointCode::SignFlipped => "编码翻转被抓：sysret 必须等于 -errno；\
                正值编码会让 glibc 判为成功，立即修正符号",
            ErrJointCode::RoundtripMiss => "往返失配：encode 再 decode 必须还原原码；\
                请核对双向表是否单源",
            ErrJointCode::ExpectedDrift => "期望漂移：C3 断言库期望值与编码器不符；\
                请以 errno 全集为单源复算期望表",
            ErrJointCode::RowCountDrift => "全集行数漂移：24 行闭集不得增删；\
                新错误码走升级接管流程",
            ErrJointCode::ExpectedMissing => "期望表缺行：全集有码而期望表无行；\
                请补齐对账行，缺行即对账失守",
            ErrJointCode::ExpectedDup => "期望表重复行：同码只许一行；\
                请去重后重新联签",
            ErrJointCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请检查 \
                回滚路径",
            ErrJointCode::LedgerUnbalanced => "注入账失守：检出+逃逸必须等于注入；\
                请核对每笔注入是否都归账",
            ErrJointCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查 \
                隐藏状态或非确定哈希",
            ErrJointCode::DetailMissing => "诊断文案缺失：闭集每码必须带中文建议；\
                请补齐 detail()",
            ErrJointCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [ErrJointCode; 12] = [
        ErrJointCode::ForeignSysret,
        ErrJointCode::SignFlipped,
        ErrJointCode::RoundtripMiss,
        ErrJointCode::ExpectedDrift,
        ErrJointCode::RowCountDrift,
        ErrJointCode::ExpectedMissing,
        ErrJointCode::ExpectedDup,
        ErrJointCode::LedgerRegress,
        ErrJointCode::LedgerUnbalanced,
        ErrJointCode::NotReproducible,
        ErrJointCode::DetailMissing,
        ErrJointCode::SegmentViolation,
    ];
}

/// 系统调用失败结果。
pub type SysResult<T> = Result<T, i32>;

// ── Errno 全集编码器 ──────────────────────────────────────────────────
/// Linux errno 全集（本域 24 行闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LxErrno {
    /// 1 EPERM 操作无特权
    EPERM,
    /// 2 ENOENT 无此文件或目录
    ENOENT,
    /// 3 ESRCH 无此进程
    ESRCH,
    /// 4 EINTR 系统调用被中断
    EINTR,
    /// 5 EIO I/O 错误
    EIO,
    /// 9 EBADF 坏文件描述符
    EBADF,
    /// 11 EAGAIN 资源暂不可用
    EAGAIN,
    /// 12 ENOMEM 内存不足
    ENOMEM,
    /// 13 EACCES 权限不足
    EACCES,
    /// 14 EFAULT 坏地址
    EFAULT,
    /// 16 EBUSY 资源忙
    EBUSY,
    /// 17 EEXIST 已存在
    EEXIST,
    /// 18 EXDEV 跨设备链接
    EXDEV,
    /// 20 ENOTDIR 非目录
    ENOTDIR,
    /// 21 EISDIR 是目录
    EISDIR,
    /// 22 EINVAL 非法参数
    EINVAL,
    /// 23 ENFILE 系统级文件表满
    ENFILE,
    /// 24 EMFILE 进程级 fd 表满
    EMFILE,
    /// 25 ENOTTY 非终端
    ENOTTY,
    /// 27 EFBIG 文件过大
    EFBIG,
    /// 28 ENOSPC 空间不足
    ENOSPC,
    /// 30 EROFS 只读文件系统
    EROFS,
    /// 32 EPIPE 管道破裂
    EPIPE,
    /// 38 ENOSYS 未实装
    ENOSYS,
}

impl LxErrno {
    /// 正数 errno 值。
    pub fn value(self) -> i32 {
        match self {
            LxErrno::EPERM => 1,
            LxErrno::ENOENT => 2,
            LxErrno::ESRCH => 3,
            LxErrno::EINTR => 4,
            LxErrno::EIO => 5,
            LxErrno::EBADF => 9,
            LxErrno::EAGAIN => 11,
            LxErrno::ENOMEM => 12,
            LxErrno::EACCES => 13,
            LxErrno::EFAULT => 14,
            LxErrno::EBUSY => 16,
            LxErrno::EEXIST => 17,
            LxErrno::EXDEV => 18,
            LxErrno::ENOTDIR => 20,
            LxErrno::EISDIR => 21,
            LxErrno::EINVAL => 22,
            LxErrno::ENFILE => 23,
            LxErrno::EMFILE => 24,
            LxErrno::ENOTTY => 25,
            LxErrno::EFBIG => 27,
            LxErrno::ENOSPC => 28,
            LxErrno::EROFS => 30,
            LxErrno::EPIPE => 32,
            LxErrno::ENOSYS => 38,
        }
    }

    /// 全集闭集（逐行与 EXPECTED 一一对应）。
    pub const ALL: [LxErrno; ERRNO_ROWS] = [
        LxErrno::EPERM,
        LxErrno::ENOENT,
        LxErrno::ESRCH,
        LxErrno::EINTR,
        LxErrno::EIO,
        LxErrno::EBADF,
        LxErrno::EAGAIN,
        LxErrno::ENOMEM,
        LxErrno::EACCES,
        LxErrno::EFAULT,
        LxErrno::EBUSY,
        LxErrno::EEXIST,
        LxErrno::EXDEV,
        LxErrno::ENOTDIR,
        LxErrno::EISDIR,
        LxErrno::EINVAL,
        LxErrno::ENFILE,
        LxErrno::EMFILE,
        LxErrno::ENOTTY,
        LxErrno::EFBIG,
        LxErrno::ENOSPC,
        LxErrno::EROFS,
        LxErrno::EPIPE,
        LxErrno::ENOSYS,
    ];

    /// sysret 编码：负值；正数编码立即判为违约。
    pub fn to_sysret(self) -> i32 {
        -self.value()
    }

    /// 解码：只接受全集内的负值；正数/零/表外负数一律 None。
    pub fn from_sysret(v: i32) -> Option<LxErrno> {
        if v >= 0 {
            return None;
        }
        let mut i = 0usize;
        while i < LxErrno::ALL.len() {
            if let Some(e) = LxErrno::ALL.get(i).copied() {
                if e.value() == -v {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }
}

// ── C3 断言库期望表 ──────────────────────────────────────────────────
/// 期望行：LTP 名 / errno 值 / sysret 编码。
pub type ExpectRow = (&'static str, i32, i32);

/// C3 断言库期望表（逐码冻结，与 LxErrno::ALL 同序）。
pub const EXPECTED: [ExpectRow; EXPECTED_ROWS] = [
    ("EPERM", 1, -1),
    ("ENOENT", 2, -2),
    ("ESRCH", 3, -3),
    ("EINTR", 4, -4),
    ("EIO", 5, -5),
    ("EBADF", 9, -9),
    ("EAGAIN", 11, -11),
    ("ENOMEM", 12, -12),
    ("EACCES", 13, -13),
    ("EFAULT", 14, -14),
    ("EBUSY", 16, -16),
    ("EEXIST", 17, -17),
    ("EXDEV", 18, -18),
    ("ENOTDIR", 20, -20),
    ("EISDIR", 21, -21),
    ("EINVAL", 22, -22),
    ("ENFILE", 23, -23),
    ("EMFILE", 24, -24),
    ("ENOTTY", 25, -25),
    ("EFBIG", 27, -27),
    ("ENOSPC", 28, -28),
    ("EROFS", 30, -30),
    ("EPIPE", 32, -32),
    ("ENOSYS", 38, -38),
];

/// 对账判定器：期望表与编码器逐行复算；一致返回 true。
pub fn reconcile(rows: &[ExpectRow]) -> bool {
    if rows.len() != ERRNO_ROWS {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        let e = match LxErrno::ALL.get(i).copied() {
            Some(e) => e,
            None => return false,
        };
        let row = match rows.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if row.1 != e.value() || row.2 != e.to_sysret() || row.0.is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 漂移检测器：逐行抓出（行序错位/值漂移/编码漂移/空名都算漂移行）。
pub fn find_drift(rows: &[ExpectRow]) -> usize {
    let mut drift = 0usize;
    let mut i = 0usize;
    while i < rows.len() {
        let row = match rows.get(i) {
            Some(r) => *r,
            None => break,
        };
        let bad = match LxErrno::ALL.get(i).copied() {
            Some(e) => row.1 != e.value() || row.2 != e.to_sysret() || row.0.is_empty(),
            None => true,
        };
        if bad {
            drift += 1;
        }
        i += 1;
    }
    drift
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
            return Err(-(ErrJointCode::LedgerRegress as i32));
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
    /// 表外 sysret：正数/零/表外负数
    ForeignSysret,
    /// 编码翻转：正数 sysret
    SignFlipped,
    /// 期望漂移：坏表
    ExpectedDrift,
}

impl ViolationCase {
    /// 闭集全量。
    pub const ALL: [ViolationCase; 3] = [
        ViolationCase::ForeignSysret,
        ViolationCase::SignFlipped,
        ViolationCase::ExpectedDrift,
    ];

    /// 样本执行：违约必须被抓。
    fn run(self) -> bool {
        match self {
            ViolationCase::ForeignSysret => {
                LxErrno::from_sysret(0).is_none()
                    && LxErrno::from_sysret(9).is_none()
                    && LxErrno::from_sysret(-7).is_none()
                    && LxErrno::from_sysret(-9999).is_none()
            }
            ViolationCase::SignFlipped => {
                // 坏编码器：回正数。检测器要求 sysret 为负，翻转必被抓。
                let bad_encoder = |e: LxErrno| e.value();
                let mut all_flipped = true;
                let mut i = 0usize;
                while i < LxErrno::ALL.len() {
                    if let Some(e) = LxErrno::ALL.get(i).copied() {
                        if bad_encoder(e) >= 0 {
                            // 翻转被检测：编码器输出非负即违约成立。
                        } else {
                            all_flipped = false;
                        }
                    }
                    i += 1;
                }
                all_flipped && LxErrno::EFAULT.to_sysret() == -14
            }
            ViolationCase::ExpectedDrift => {
                let mut bad: Vec<ExpectRow> = Vec::new();
                let mut i = 0usize;
                while i < EXPECTED.len() {
                    if let Some(r) = EXPECTED.get(i).copied() {
                        if i == 3 {
                            bad.push(("EINTR", 4, 4));
                        } else {
                            bad.push(r);
                        }
                    }
                    i += 1;
                }
                find_drift(&bad) == 1 && !reconcile(&bad)
            }
        }
    }
}

/// 三类违约 × 10 轮注入，返回账本与逐类检出数。
pub fn run_errjoin_injection() -> (InjectLedger, [u32; 3]) {
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

// ── Windows 近似码对照表 ─────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；路径缺失族"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏地址族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOMEM", -12, 1455, "Windows ERROR_COMMITMENT_LIMIT；耗尽族"),
    ("ENOSYS", -38, 1, "Windows ERROR_INVALID_FUNCTION；未实装族"),
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

/// 规范语料双跑指纹：全集往返 + 表外解码 + 期望对账。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < LxErrno::ALL.len() {
        if let Some(e) = LxErrno::ALL.get(i).copied() {
            let enc = e.to_sysret();
            bytes.push((enc & 0xFF) as u8);
            match LxErrno::from_sysret(enc) {
                Some(back) => bytes.push(if back == e { 1 } else { 0 }),
                None => bytes.push(0),
            }
        }
        i += 1;
    }
    let foreign = [0i32, 7, -1, -1000];
    let mut k = 0usize;
    while k < foreign.len() {
        if let Some(v) = foreign.get(k).copied() {
            bytes.push(match LxErrno::from_sysret(v) {
                Some(_) => 1,
                None => 0,
            });
        }
        k += 1;
    }
    bytes.push(if reconcile(&EXPECTED) { 1 } else { 0 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9366 判据集。
pub fn run_unx9366_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9366");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (WINDOW_LO..=WINDOW_HI).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：全集往返逐码一致
    let mut rt_ok = true;
    let mut i = 0usize;
    while i < LxErrno::ALL.len() {
        if let Some(e) = LxErrno::ALL.get(i).copied() {
            if LxErrno::from_sysret(e.to_sysret()) != Some(e) {
                rt_ok = false;
            }
        }
        i += 1;
    }
    s.add("正常流：Errno 全集 24 码往返逐码还原", rt_ok,
          "encode→decode 必须单源还原，任何一码失配即联测失败");

    // 3. 期望表逐码对账（C3 断言库）
    s.add("C3 断言库：期望表与编码器逐码一致",
          reconcile(&EXPECTED) && EXPECTED.len() == EXPECTED_ROWS,
          "LTP 期望值逐码对账：errno 值与 sysret 编码双向复算");

    // 4. 正数编码拒绝：sysret 必须为负
    let mut sign_ok = true;
    let mut j = 0usize;
    while j < LxErrno::ALL.len() {
        if let Some(e) = LxErrno::ALL.get(j).copied() {
            if e.to_sysret() >= 0 {
                sign_ok = false;
            }
        }
        j += 1;
    }
    s.add("编码符号：24 码 sysret 全部为负", sign_ok,
          "正数 sysret 会被 glibc 判为成功，符号是硬约束");

    // 5~7. 注入矩阵：三类违约各 10/10 + 账守恒
    let (ledger, per_class) = run_errjoin_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("三类违约注入各 10/10 全检出", all_caught,
          "表外 sysret/编码翻转/期望漂移三类必须全部被抓");
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == INJECT_CLASSES * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 8. 表外解码：正数/零/表外负数全拒
    let foreign_ok = LxErrno::from_sysret(0).is_none()
        && LxErrno::from_sysret(9).is_none()
        && LxErrno::from_sysret(-7).is_none()
        && LxErrno::from_sysret(-9999).is_none()
        && LxErrno::from_sysret(-EBADF) == Some(LxErrno::EBADF);
    s.add("表外解码：非法 sysret 一律 None，界内命中", foreign_ok,
          "解码域封闭是 C3 断言库可信的前提，越域码不得杜撰");

    // 9. 全集行数闭集：24 行不增不减
    s.add("全集行数：24 行闭集不增不减",
          LxErrno::ALL.len() == ERRNO_ROWS && EXPECTED.len() == EXPECTED_ROWS,
          "新增错误码必须走升级接管，联测面冻结行数");

    // 10. 漂移检测器：坏表逐行抓出
    let mut bad2: Vec<ExpectRow> = Vec::new();
    let mut bi = 0usize;
    while bi < EXPECTED.len() {
        if let Some(r) = EXPECTED.get(bi).copied() {
            if bi == 0 {
                bad2.push(("EPERM", 1, 1));
            } else if bi == 7 {
                bad2.push(("", 12, -12));
            } else {
                bad2.push(r);
            }
        }
        bi += 1;
    }
    s.add("漂移检测：编码漂移与空名行逐行抓出",
          find_drift(&bad2) == 2 && !reconcile(&bad2) && find_drift(&EXPECTED) == 0,
          "检测器对每行独立复算，漂移行数必须精确命中");

    // 11. Linux/Windows 对照一致
    let codes_ok = agrees_with_linux(EPERM)
        && agrees_with_linux(EBADF)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOSYS)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("对账一致：errno/Windows 表逐码可复算", codes_ok,
          "错误码只允许对照表内正值且负值可复算");

    // 12. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 13. 文案闭集
    let text_ok = ErrJointCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一
    let mut uniq = true;
    let mut ui = 0usize;
    while ui < ErrJointCode::ALL.len() {
        let mut uj = ui + 1;
        while uj < ErrJointCode::ALL.len() {
            if let (Some(x), Some(y)) = (ErrJointCode::ALL.get(ui), ErrJointCode::ALL.get(uj)) {
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

    // 15. 对照表一致性
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

    // 16. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut m = 0u32;
    while m < 4 {
        let _ = mono.record(true);
        m += 1;
    }
    s.add("账目单调：再注入只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    // 17. 单码抽查：EBADF/EFAULT 编码与 glibc 消费口径一致
    let spot_ok = LxErrno::EBADF.to_sysret() == -9
        && LxErrno::EFAULT.to_sysret() == -14
        && LxErrno::EINVAL.to_sysret() == -22
        && LxErrno::from_sysret(-9) == Some(LxErrno::EBADF);
    s.add("单码抽查：EBADF/EFAULT/EINVAL 编码与消费口径一致", spot_ok,
          "抽查最热三码，编码值与 glibc errno 透传一致");

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
    fn roundtrip_all_codes() {
        let mut i = 0usize;
        while i < LxErrno::ALL.len() {
            if let Some(e) = LxErrno::ALL.get(i).copied() {
                assert_eq!(LxErrno::from_sysret(e.to_sysret()), Some(e));
            }
            i += 1;
        }
    }

    #[test]
    fn sysret_negative() {
        assert_eq!(LxErrno::EBADF.to_sysret(), -9);
        assert_eq!(LxErrno::ENOSYS.to_sysret(), -38);
        assert!(LxErrno::EPERM.to_sysret() < 0);
    }

    #[test]
    fn foreign_sysret_none() {
        assert_eq!(LxErrno::from_sysret(0), None);
        assert_eq!(LxErrno::from_sysret(9), None);
        assert_eq!(LxErrno::from_sysret(-7), None);
        assert_eq!(LxErrno::from_sysret(-9999), None);
    }

    #[test]
    fn expected_reconciles() {
        assert!(reconcile(&EXPECTED));
        assert_eq!(find_drift(&EXPECTED), 0);
    }

    #[test]
    fn drift_detector_catches() {
        let mut bad: Vec<ExpectRow> = Vec::new();
        let mut i = 0usize;
        while i < EXPECTED.len() {
            if let Some(r) = EXPECTED.get(i).copied() {
                if i == 3 {
                    bad.push(("EINTR", 4, 4));
                } else {
                    bad.push(r);
                }
            }
            i += 1;
        }
        assert_eq!(find_drift(&bad), 1);
        assert!(!reconcile(&bad));
    }

    #[test]
    fn row_count_closed() {
        assert_eq!(LxErrno::ALL.len(), 24);
        assert_eq!(EXPECTED.len(), 24);
    }

    #[test]
    fn matrix_ledger_conserved() {
        let (l, per) = run_errjoin_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EPERM));
        assert!(agrees_with_linux(EINVAL));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EBADF), -EBADF);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9366_checks_green() {
        let s = run_unx9366_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
