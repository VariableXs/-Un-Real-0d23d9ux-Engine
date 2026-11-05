//! UNX-F9267 EPERM 特权操作矩阵（root-only 全集） · 0x7B040段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9267-J1 root-only 操作全集注入判据过，非 root 全 EPERM
//! 逐格检出，CAP 消费声明格过。
//!
//! 本卡定制域：root 豁免面矩阵——root 绕过 rwx 对象位检查但执行仍需任一
//! x 位存在；chown/chmod root 豁免；root 也不能越 fd 域界（EBADF 照报），
//! 逐格落账。
//!
//! SEG_BASE = 0x7B040：诊断码闭集说明——本模块全部诊断码由
//! `RootCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入豁免账。
//!
//! 结构清单：
//!   ① `RootCred`/`PermObj`：主体与对象模型（uid + mode）。
//!   ② `judge_rw`/`judge_exec`：r/w 豁免与 x 位不豁免判定器。
//!   ③ `judge_chown`/`judge_chmod`：root 豁免的特权操作判定器。
//!   ④ `RootFdTable`：fd 域界——root 坏 fd 照报 EBADF（负值/未开/已关/
//!      越界四类闭集）。
//!   ⑤ `ExemptLedger`：注入账（注入=检出+逃逸，逃逸恒 0）。
//!   ⑥ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9267 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7B040;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// fd 域界：动态槽上限。
pub const FD_MAX: usize = 16;

/// Linux errno 数值（正数口径）。
pub const EPERM: i32 = 1;
pub const EACCES: i32 = 13;
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RootCode {
    /// root 对全无 x 位对象执行被拒（x 位不豁免）
    RootExecNoX,
    /// root 坏 fd 未豁免（fd 域界照报 EBADF）
    RootBadFdNotExempt,
    /// 非 root 特权操作 EPERM
    NotOwnerEperm,
    /// 负 fd
    FdNegative,
    /// 未打开 fd
    FdUnopened,
    /// 已关闭 fd
    FdClosed,
    /// fd 越上界
    FdBeyond,
    /// 非 root 读对象位缺口 EACCES
    EaccesGap,
    /// 豁免账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl RootCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RootCode::RootExecNoX => "root 执行全无 x 位对象被拒：root 豁免 r/w \
                对象位但不创造 x 位；请 chmod 补任一 x 位",
            RootCode::RootBadFdNotExempt => "root 坏 fd 未豁免：fd 域界独立于\
                豁免面，负值/未开/已关/越界照报 EBADF",
            RootCode::NotOwnerEperm => "非 root 特权操作被拒：chown/chmod 等特权\
                操作只有 root 或属主可走，其余一律 EPERM",
            RootCode::FdNegative => "负 fd 被拒：fd 小于 0 属非法号段，root 也\
                不得豁免",
            RootCode::FdUnopened => "未打开 fd 被拒：槽位空闲即 EBADF，与身份\
                无关",
            RootCode::FdClosed => "已关闭 fd 被拒：关闭后槽位失效，root 亦不豁免",
            RootCode::FdBeyond => "fd 越上界被拒：号超出动态槽上限即 EBADF",
            RootCode::EaccesGap => "非 root 对象位缺口被拒：读/写/执行缺位一律 \
                EACCES，root 之外无豁免",
            RootCode::LedgerRegress => "豁免账倒退被拒：计数只增不减；请排查\
                回滚路径误改累计值",
            RootCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            RootCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [RootCode; 11] = [
        RootCode::RootExecNoX,
        RootCode::RootBadFdNotExempt,
        RootCode::NotOwnerEperm,
        RootCode::FdNegative,
        RootCode::FdUnopened,
        RootCode::FdClosed,
        RootCode::FdBeyond,
        RootCode::EaccesGap,
        RootCode::LedgerRegress,
        RootCode::NotReproducible,
        RootCode::SegmentViolation,
    ];
}

// ── 模型 ─────────────────────────────────────────────────────────────
/// 主体：root 或普通用户。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RootCred {
    /// uid（0=root）
    pub uid: u32,
}

impl RootCred {
    /// root。
    pub const fn root() -> RootCred {
        RootCred { uid: 0 }
    }

    /// 普通用户。
    pub const fn user(uid: u32) -> RootCred {
        RootCred { uid }
    }

    /// 是否 root。
    pub fn is_root(&self) -> bool {
        self.uid == 0
    }
}

/// 对象：mode（低 9 位）+ 属主。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermObj {
    /// 属主 uid
    pub uid: u32,
    /// mode 低 9 位
    pub mode: u32,
}

impl PermObj {
    /// 三组任一 x 位存在。
    pub fn any_x(&self) -> bool {
        self.mode & 0o111 != 0
    }

    /// 取某组 3 位（0=owner 1=group 2=other）。
    pub fn class_bits(&self, class: usize) -> u32 {
        match class {
            0 => (self.mode >> 6) & 0o7,
            1 => (self.mode >> 3) & 0o7,
            _ => self.mode & 0o7,
        }
    }
}

/// 访问方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Direction {
    /// 读
    Read,
    /// 写
    Write,
    /// 执行
    Exec,
}

// ── 判定器 ───────────────────────────────────────────────────────────
/// judge_rw：root 豁免 r/w 对象位；非 root 按归属类位裁定。
pub fn judge_rw(cred: &RootCred, obj: &PermObj, dir: Direction) -> Result<(), i32> {
    if cred.is_root() {
        return Ok(());
    }
    let class = if cred.uid == obj.uid {
        0
    } else {
        2
    };
    let bits = obj.class_bits(class);
    let need = match dir {
        Direction::Read => 0o4,
        Direction::Write => 0o2,
        Direction::Exec => 0o1,
    };
    if bits & need == need {
        Ok(())
    } else {
        Err(EACCES)
    }
}

/// judge_exec：root 也需任一 x 位存在；非 root 按归属类 x 位。
pub fn judge_exec(cred: &RootCred, obj: &PermObj) -> Result<(), i32> {
    if cred.is_root() {
        if obj.any_x() {
            return Ok(());
        }
        return Err(EACCES);
    }
    let class = if cred.uid == obj.uid { 0 } else { 2 };
    let bits = obj.class_bits(class);
    if bits & 0o1 == 0o1 {
        Ok(())
    } else {
        Err(EACCES)
    }
}

/// judge_chown：root 豁免；属主豁免；其余 EPERM。
pub fn judge_chown(cred: &RootCred, obj: &PermObj) -> Result<(), i32> {
    if cred.is_root() || cred.uid == obj.uid {
        Ok(())
    } else {
        Err(EPERM)
    }
}

/// judge_chmod：root 豁免；属主豁免；其余 EPERM。
pub fn judge_chmod(cred: &RootCred, obj: &PermObj) -> Result<(), i32> {
    if cred.is_root() || cred.uid == obj.uid {
        Ok(())
    } else {
        Err(EPERM)
    }
}

// ── fd 域界（root 不豁免） ───────────────────────────────────────────
/// 槽位状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲
    Free,
    /// 打开
    Open,
    /// 已关闭
    Closed,
}

/// root 的 fd 表：fd 域界与身份无关。
pub struct RootFdTable {
    slots: [SlotState; FD_MAX],
    /// 累计分配
    pub allocs: u32,
    /// 累计关闭
    pub closes: u32,
}

impl RootFdTable {
    /// 构造空表。
    pub const fn new() -> RootFdTable {
        RootFdTable { slots: [SlotState::Free; FD_MAX], allocs: 0, closes: 0 }
    }

    /// 打开：取首个非 Open 槽。
    pub fn open(&mut self) -> Result<usize, i32> {
        let mut i = 0usize;
        while i < FD_MAX {
            if let Some(s) = self.slots.get_mut(i) {
                if *s != SlotState::Open {
                    *s = SlotState::Open;
                    self.allocs = self.allocs.saturating_add(1);
                    return Ok(i);
                }
            }
            i += 1;
        }
        Err(24) // EMFILE 占位
    }

    /// 关闭：坏 fd（负值/越界/未开/已关）EBADF，root 不豁免。
    pub fn close(&mut self, fd: i32) -> Result<(), i32> {
        let slot = self.slot_state(fd)?;
        if slot != SlotState::Open {
            return Err(EBADF);
        }
        if let Some(s) = self.slots.get_mut(fd as usize) {
            *s = SlotState::Closed;
        }
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }

    /// 读域界判定：fd 非法即 EBADF（root 不豁免），合法即放行。
    pub fn judge_read_fd(&self, fd: i32) -> Result<(), i32> {
        let st = self.slot_state(fd)?;
        if st == SlotState::Open {
            Ok(())
        } else {
            Err(EBADF)
        }
    }

    /// 槽位解析：负值/越界先拒（域界），返回状态。
    fn slot_state(&self, fd: i32) -> Result<SlotState, i32> {
        if fd < 0 {
            return Err(EBADF);
        }
        if fd >= FD_MAX as i32 {
            return Err(EBADF);
        }
        match self.slots.get(fd as usize) {
            Some(s) => Ok(*s),
            None => Err(EBADF),
        }
    }
}

// ── 豁免账 ───────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExemptLedger {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数
    pub escaped: u32,
}

impl ExemptLedger {
    const fn zero() -> ExemptLedger {
        ExemptLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), RootCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RootCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒判定。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

// ── 豁免面矩阵 ───────────────────────────────────────────────────────
/// 逐格矩阵：身份（root/user）× 方向（r/w/x）× 模式代表格。
/// 返回（格数, 一致格数, 漂移标记）。
pub fn run_exempt_matrix() -> (u32, u32, bool) {
    let modes = [0o000, 0o400, 0o200, 0o100, 0o644, 0o755];
    let root = RootCred::root();
    let user = RootCred::user(1000); // 属主身份
    let mut total = 0u32;
    let mut hit = 0u32;
    let mut drift = false;
    let mut mi = 0usize;
    while mi < modes.len() {
        let mode = match modes.get(mi) {
            Some(m) => *m,
            None => break,
        };
        let probe = PermObj { uid: 1000, mode };
        // root 期望：r/w 恒过；x 依任一 x 位
        // user（属主）期望：按 owner 组 3 位
        let mut d = 0usize;
        while d < 3 {
            let (want_root, want_user): (bool, bool) = match d {
                0 => (true, probe.mode & 0o400 != 0),
                1 => (true, probe.mode & 0o200 != 0),
                _ => (probe.any_x(), probe.mode & 0o100 != 0),
            };
            let got_root = match d {
                0 => judge_rw(&root, &probe, Direction::Read),
                1 => judge_rw(&root, &probe, Direction::Write),
                _ => judge_exec(&root, &probe),
            };
            let got_user = match d {
                0 => judge_rw(&user, &probe, Direction::Read),
                1 => judge_rw(&user, &probe, Direction::Write),
                _ => judge_exec(&user, &probe),
            };
            let ok_root = (got_root == Ok(())) == want_root;
            let ok_user = match got_user {
                Ok(()) => want_user,
                Err(e) => !want_user && e == EACCES,
            };
            total += 2;
            if ok_root {
                hit += 1;
            } else {
                drift = true;
            }
            if ok_user {
                hit += 1;
            } else {
                drift = true;
            }
            d += 1;
        }
        mi += 1;
    }
    (total, hit, !drift)
}

// ── 坏样本注入矩阵 ───────────────────────────────────────────────────
/// 三类违例样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRootCase {
    /// root 对全无 x 位对象执行
    RootExecNoX,
    /// root 坏 fd（负值/越界）未豁免
    RootBadFd,
    /// 非 root chown 他人文件
    UserChownEperm,
}

impl BadRootCase {
    /// 执行注入：返回是否被判定器抓到。
    pub fn probe(self) -> bool {
        let root = RootCred::root();
        match self {
            BadRootCase::RootExecNoX => {
                let obj = PermObj { uid: 1000, mode: 0o600 };
                judge_exec(&root, &obj) == Err(EACCES)
            }
            BadRootCase::RootBadFd => {
                let mut t = RootFdTable::new();
                let neg = t.judge_read_fd(-1) == Err(EBADF);
                let beyond = t.judge_read_fd(FD_MAX as i32) == Err(EBADF);
                let closed = {
                    let fd = t.open().unwrap_or(usize::MAX);
                    if fd != usize::MAX {
                        let _ = t.close(fd as i32);
                        t.judge_read_fd(fd as i32) == Err(EBADF)
                    } else {
                        false
                    }
                };
                let unopened = t.judge_read_fd(5) == Err(EBADF);
                neg && beyond && closed && unopened
            }
            BadRootCase::UserChownEperm => {
                let user = RootCred::user(2000);
                let obj = PermObj { uid: 1000, mode: 0o777 };
                judge_chown(&user, &obj) == Err(EPERM)
                    && judge_chmod(&user, &obj) == Err(EPERM)
            }
        }
    }

    /// 闭集全量。
    pub const ALL: [BadRootCase; 3] = [
        BadRootCase::RootExecNoX,
        BadRootCase::RootBadFd,
        BadRootCase::UserChownEperm,
    ];
}

/// 执行注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_root_matrix() -> (ExemptLedger, [u32; 3]) {
    let mut ledger = ExemptLedger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < BadRootCase::ALL.len() {
        if let Some(case) = BadRootCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.probe();
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

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；特权缺失族"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；fd 域界族"),
    ("EACCES", 13, 5, "Windows ERROR_ACCESS_DENIED；对象位缺口族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
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

/// 规范语料双跑指纹：豁免矩阵 + fd 域界账 + 注入账入哈希。
pub fn corpus_fingerprint() -> u32 {
    let (total, hit, no_drift) = run_exempt_matrix();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((total & 0xFF) as u8);
    bytes.push((hit & 0xFF) as u8);
    bytes.push(if no_drift { 1 } else { 0 });
    let mut t = RootFdTable::new();
    let _ = t.open();
    let _ = t.open();
    let _ = t.close(0);
    bytes.push(t.allocs as u8);
    bytes.push(t.closes as u8);
    bytes.push(if t.judge_read_fd(1).is_ok() { 1 } else { 0 });
    bytes.push(if t.judge_read_fd(0) == Err(EBADF) { 1 } else { 0 });
    let (ledger, _) = run_bad_root_matrix();
    bytes.push(ledger.detected as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9267 判据集。
pub fn run_unx9267_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9267");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7B040 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：root 对 0o000 对象读/写豁免
    let root = RootCred::root();
    let obj0 = PermObj { uid: 1000, mode: 0o000 };
    s.add("正常流：root 对 0o000 读/写豁免通过",
          judge_rw(&root, &obj0, Direction::Read).is_ok()
              && judge_rw(&root, &obj0, Direction::Write).is_ok(),
          "root 绕过 rwx 对象位，r/w 逐格放行");

    // 3. x 位不豁免：root 对全无 x 位对象执行拒绝
    s.add("豁免边界：root 对 0o000 执行 EACCES（x 位不豁免）",
          judge_exec(&root, &obj0) == Err(EACCES),
          "root 不创造 x 位，任一 x 存在才放行执行");

    // 4. 任一 x 位存在：root 执行放行（other 位 x 即可）
    let obj1 = PermObj { uid: 1000, mode: 0o001 };
    s.add("豁免边界：0o001（仅 other x）root 执行放行",
          judge_exec(&root, &obj1).is_ok(),
          "三组任一 x 存在即满足 root 执行豁免面");

    // 5. 非 root 对象位缺口逐方向 EACCES
    let user = RootCred::user(2000);
    s.add("非 root：0o000 对象读/写/执行全 EACCES",
          judge_rw(&user, &obj0, Direction::Read) == Err(EACCES)
              && judge_rw(&user, &obj0, Direction::Write) == Err(EACCES)
              && judge_exec(&user, &obj0) == Err(EACCES),
          "root 之外无豁免，缺位逐方向拒绝");

    // 6. chown/chmod root 豁免、属主豁免、他人 EPERM
    let obj777 = PermObj { uid: 1000, mode: 0o777 };
    let owner = RootCred::user(1000);
    let stranger = RootCred::user(2000);
    s.add("特权操作：root/属主豁免、他人 chown/chmod 全 EPERM",
          judge_chown(&root, &obj777).is_ok()
              && judge_chmod(&root, &obj777).is_ok()
              && judge_chown(&owner, &obj777).is_ok()
              && judge_chown(&stranger, &obj777) == Err(EPERM)
              && judge_chmod(&stranger, &obj777) == Err(EPERM),
          "特权面逐格：root 与属主放行，其余 EPERM");

    // 7. fd 域界：root 四类坏 fd 照报 EBADF
    let mut t = RootFdTable::new();
    let neg = t.judge_read_fd(-1) == Err(EBADF);
    let beyond = t.judge_read_fd(FD_MAX as i32) == Err(EBADF);
    let unopened = t.judge_read_fd(3) == Err(EBADF);
    let fd0 = t.open().unwrap_or(usize::MAX);
    let closed_ok = if fd0 != usize::MAX {
        let _ = t.close(fd0 as i32);
        t.judge_read_fd(fd0 as i32) == Err(EBADF)
    } else {
        false
    };
    s.add("fd 域界：root 负值/越界/未开/已关四类照报 EBADF",
          neg && beyond && unopened && closed_ok,
          "fd 域界独立于豁免面，root 不得越界");

    // 8. fd 正常流：root 合法 fd 放行
    let fd1 = t.open().unwrap_or(usize::MAX);
    s.add("fd 正常流：root 合法打开槽读放行",
          fd1 != usize::MAX && t.judge_read_fd(fd1 as i32).is_ok(),
          "域界只拦坏 fd，合法槽零干扰");

    // 9. 豁免面逐格矩阵：2 身份 × 3 方向 × 6 模式 = 36 格零漂移
    let (total, hit, no_drift) = run_exempt_matrix();
    s.add("豁免矩阵：36 格全落账零漂移", total == 36 && hit == 36 && no_drift,
          "root r/w 恒过、x 依任一 x；user 按位逐格");

    // 10. 注入必抓：三类坏样本各 10/10
    let (ledger, per_class) = run_bad_root_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：root无x执行/root坏fd/非root chown各 10/10", all_caught,
          "三类违约样本必须全部被抓");

    // 11. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadRootCase::ALL.len() as u32 * INJECT_ROUNDS,
          "30 次注入必须 30 次检出、0 逃逸");

    // 12. CAP 消费声明：root 豁免面不依赖 CAP 位，EBADF/EPERM/EACCES 三码闭集
    let cap_note = agrees_with_linux(EBADF) && agrees_with_linux(EPERM)
        && agrees_with_linux(EACCES);
    s.add("CAP 消费声明：三码闭集对照表复算通过", cap_note,
          "豁免面消费的 errno 三码在对照表内精确");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EINVAL) && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集 + 码唯一
    let text_ok = RootCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    let mut uniq = true;
    let mut i15 = 0usize;
    while i15 < RootCode::ALL.len() {
        let mut j15 = i15 + 1;
        while j15 < RootCode::ALL.len() {
            if let (Some(x), Some(y)) = (RootCode::ALL.get(i15), RootCode::ALL.get(j15)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j15 += 1;
        }
        i15 += 1;
    }
    s.add("文案闭集与码唯一：11 码非空且两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文建议且段内不重复");

    // 动态明细（≤8）
    let ledger_text = format!("注入 {} / 检出 {} / 逃逸 {}",
                              ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let matrix_text = format!("矩阵 {} 格 / 一致 {}", total, hit);
    s.add_dyn("矩阵快照", total == 36 && hit == 36, &matrix_text);

    // 16. 账目单调
    let mut mono = ledger;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 4 次只增不减", mono.injected == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_rw_exempt_on_zero_bits() {
        let root = RootCred::root();
        let obj = PermObj { uid: 1000, mode: 0o000 };
        assert!(judge_rw(&root, &obj, Direction::Read).is_ok());
        assert!(judge_rw(&root, &obj, Direction::Write).is_ok());
    }

    #[test]
    fn root_exec_needs_any_x() {
        let root = RootCred::root();
        let nox = PermObj { uid: 1000, mode: 0o600 };
        assert_eq!(judge_exec(&root, &nox), Err(EACCES));
        let hasx = PermObj { uid: 1000, mode: 0o001 };
        assert!(judge_exec(&root, &hasx).is_ok());
    }

    #[test]
    fn user_bits_enforced() {
        let user = RootCred::user(2000);
        let obj = PermObj { uid: 1000, mode: 0o600 };
        assert_eq!(judge_rw(&user, &obj, Direction::Read), Err(EACCES));
        assert_eq!(judge_rw(&user, &obj, Direction::Write), Err(EACCES));
        let other_r = PermObj { uid: 1000, mode: 0o604 };
        assert!(judge_rw(&user, &other_r, Direction::Read).is_ok());
    }

    #[test]
    fn owner_class_rw() {
        let owner = RootCred::user(1000);
        let obj = PermObj { uid: 1000, mode: 0o600 };
        assert!(judge_rw(&owner, &obj, Direction::Read).is_ok());
        assert!(judge_rw(&owner, &obj, Direction::Write).is_ok());
        assert_eq!(judge_exec(&owner, &obj), Err(EACCES));
    }

    #[test]
    fn chown_chmod_privileged() {
        let root = RootCred::root();
        let owner = RootCred::user(1000);
        let stranger = RootCred::user(2000);
        let obj = PermObj { uid: 1000, mode: 0o600 };
        assert!(judge_chown(&root, &obj).is_ok());
        assert!(judge_chmod(&root, &obj).is_ok());
        assert!(judge_chown(&owner, &obj).is_ok());
        assert_eq!(judge_chown(&stranger, &obj), Err(EPERM));
        assert_eq!(judge_chmod(&stranger, &obj), Err(EPERM));
    }

    #[test]
    fn root_bad_fd_not_exempt() {
        let mut t = RootFdTable::new();
        assert_eq!(t.judge_read_fd(-1), Err(EBADF));
        assert_eq!(t.judge_read_fd(FD_MAX as i32), Err(EBADF));
        assert_eq!(t.judge_read_fd(0), Err(EBADF));
    }

    #[test]
    fn root_closed_fd_ebadf() {
        let mut t = RootFdTable::new();
        let fd = t.open().unwrap_or(usize::MAX);
        assert!(fd != usize::MAX);
        assert!(t.judge_read_fd(fd as i32).is_ok());
        assert!(t.close(fd as i32).is_ok());
        assert_eq!(t.judge_read_fd(fd as i32), Err(EBADF));
        assert_eq!(t.close(fd as i32), Err(EBADF));
    }

    #[test]
    fn fd_table_accounts() {
        let mut t = RootFdTable::new();
        let mut i = 0;
        while i < 4 {
            let fd = t.open().unwrap_or(usize::MAX);
            assert!(fd != usize::MAX);
            let _ = t.close(fd as i32);
            i += 1;
        }
        assert_eq!(t.allocs, 4);
        assert_eq!(t.closes, 4);
    }

    #[test]
    fn exempt_matrix_zero_drift() {
        let (total, hit, no_drift) = run_exempt_matrix();
        assert_eq!(total, 36);
        assert_eq!(hit, 36);
        assert!(no_drift);
    }

    #[test]
    fn injection_matrix_all_caught() {
        let (l, per) = run_bad_root_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(1));
        assert!(agrees_with_linux(9));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9267_checks_green() {
        let s = run_unx9267_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
