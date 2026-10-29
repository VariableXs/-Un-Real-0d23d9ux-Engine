//! unx4124_fmodeops — UNX-F4124 f_mode 读/写/执行位到 f_op 约束（诊断段 0x9160）。
//!
//! 判据原文：UNX-F4124-J1 只读打开调 write 注入返回 EBADF 10/10 次，只写打开调
//!           read 同理——f_mode 与 f_op 白名单约束矩阵全过。
//!   J2 f_mode 与 f_op 二重门判定序固定（EBADF 先于缺实现）。
//!   J3 三账（allow/deny/unimplemented）与查询总数恒等。
//!   J4 4x6 约束矩阵 24 格逐格可复算。
//! 设计纪律：
//!   1. f_mode 位按 Linux include/linux/fs.h 原值取定：FMODE_READ=0x1、
//!      FMODE_WRITE=0x2、FMODE_LSEEK=0x4、FMODE_EXEC=0x20。一位一义，组合按位或，
//!      位间不重叠，闭集外任何位都判非法（不做「忽略未知位」的宽容处理）。
//!   2. 判定为「二重门」且门序固定：先查 f_mode 是否授予该操作（不授→EBADF 9），
//!      再查 f_op 是否提供该操作（缺→不实现面）。门序与 vfs_read/vfs_write 一致
//!      （先检 f_mode，再检 f_op 空指针），故 EBADF 优先于缺实现暴露。
//!   3. 每次查询必入三账之一（allow/deny/unimplemented），三账之和恒等于查询总数；
//!      拒收不是「不算数」，拒收本身就是判据的观测对象。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9160 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9160;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BADF: u16 = SEG_BASE + 1; // 对应 EBADF(9)：f_mode 未授予该操作
const E_MODE_EMPTY: u16 = SEG_BASE + 2; // 打开模式为空（无任何访问位）
const E_MODE_UNKNOWN: u16 = SEG_BASE + 3; // f_mode 出现闭集外位
const E_FOP_NONE: u16 = SEG_BASE + 4; // f_op 未提供该操作（不实现面）
const E_OP_UNKNOWN: u16 = SEG_BASE + 5; // 操作码不在闭集
const E_ACCT: u16 = SEG_BASE + 6; // allow+deny+unimpl 与查询总数不平
const E_REASSEMB: u16 = SEG_BASE + 7; // 重复装配或重复释放
const E_RELEASED: u16 = SEG_BASE + 8; // 已释放句柄再查询

/// EBADF 的 errno 原值（POSIX errno.h），判据要求按此值断言。
pub const EBADF_ERRNO: u16 = 9;

// ── f_mode 位常量（Linux include/linux/fs.h，逐位原值）────────
pub const FMODE_READ: u32 = 0x0000_0001; // O_RDONLY / O_RDWR 打开后置位
pub const FMODE_WRITE: u32 = 0x0000_0002; // O_WRONLY / O_RDWR 打开后置位
pub const FMODE_LSEEK: u32 = 0x0000_0004; // 可 lseek（常规文件默认给）
pub const FMODE_EXEC: u32 = 0x0000_0020; // execve 路径打开时置位
/// f_mode 闭集：闭集外位一律判 E_MODE_UNKNOWN。
pub const FMODE_VALID: u32 = FMODE_READ | FMODE_WRITE | FMODE_LSEEK | FMODE_EXEC;

// ── O_ACCMODE 取值（open(2)）────────────────────────────────
pub const O_ACCMODE: u32 = 0o3;
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;

// ── f_op 能力位（白名单，一位对应一个 file_operations 成员）──
pub const FOP_READ: u32 = 0x0001;
pub const FOP_WRITE: u32 = 0x0002;
pub const FOP_LSEEK: u32 = 0x0004;
pub const FOP_MMAP: u32 = 0x0008;
pub const FOP_IOCTL: u32 = 0x0010;
pub const FOP_EXEC: u32 = 0x0020; // exec 通路（read_iter/mmap 侧间接满足，单列便于断言）

/// 文件操作闭集（6 项）：约束矩阵的行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOp {
    Read = 0,
    Write = 1,
    Lseek = 2,
    Mmap = 3,
    Ioctl = 4,
    Exec = 5,
}

pub const OP_COUNT: usize = 6;

impl FileOp {
    /// 操作码 → 闭集元素；越界即 None（拒收而非裁剪）。
    pub fn from_code(c: u8) -> Option<FileOp> {
        match c {
            0 => Some(FileOp::Read),
            1 => Some(FileOp::Write),
            2 => Some(FileOp::Lseek),
            3 => Some(FileOp::Mmap),
            4 => Some(FileOp::Ioctl),
            5 => Some(FileOp::Exec),
            _ => None,
        }
    }

    /// 该操作对 f_mode 的最小授权集（任一命中即授）。
    pub fn need_mode(self) -> u32 {
        match self {
            FileOp::Read => FMODE_READ,
            FileOp::Write => FMODE_WRITE,
            FileOp::Lseek => FMODE_LSEEK,
            // mmap 需读或写之一（PROT_READ→FMODE_READ，PROT_WRITE→FMODE_WRITE）
            FileOp::Mmap => FMODE_READ | FMODE_WRITE,
            FileOp::Ioctl => FMODE_READ | FMODE_WRITE,
            // 执行只认读权限：写-only 打开的文件不可 exec
            FileOp::Exec => FMODE_READ,
        }
    }

    /// 该操作对 f_op 白名单的最小能力集。
    pub fn need_fop(self) -> u32 {
        match self {
            FileOp::Read => FOP_READ,
            FileOp::Write => FOP_WRITE,
            FileOp::Lseek => FOP_LSEEK,
            FileOp::Mmap => FOP_MMAP,
            FileOp::Ioctl => FOP_IOCTL,
            FileOp::Exec => FOP_EXEC,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FileOp::Read => "read",
            FileOp::Write => "write",
            FileOp::Lseek => "llseek",
            FileOp::Mmap => "mmap",
            FileOp::Ioctl => "ioctl",
            FileOp::Exec => "exec",
        }
    }
}

/// 句柄生命周期状态机：装配 → 活跃 → 释放。释放是终态，不可复活。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandleState {
    Assembled,
    Active,
    Released,
}

/// f_op 白名单：由 fs 嫁接侧在 open 时填充的能力位集合。
#[derive(Clone, Copy)]
pub struct FopCaps {
    /// 能力位（FOP_* 的或）
    pub bits: u32,
}

impl FopCaps {
    pub fn new(bits: u32) -> FopCaps {
        FopCaps { bits }
    }

    /// 常规文件：全能力。
    pub fn regular_file() -> FopCaps {
        FopCaps {
            bits: FOP_READ | FOP_WRITE | FOP_LSEEK | FOP_MMAP | FOP_IOCTL | FOP_EXEC,
        }
    }

    /// 只读设备：无 write 通路（write 必落 E_FOP_NONE）。
    pub fn read_only_dev() -> FopCaps {
        FopCaps {
            bits: FOP_READ | FOP_LSEEK | FOP_IOCTL | FOP_EXEC,
        }
    }

    pub fn has(&self, op: FileOp) -> bool {
        (self.bits & op.need_fop()) != 0
    }
}

/// 单个打开文件对象：f_mode + f_op 白名单 + 生命周期状态。
pub struct FileCtx {
    /// f_mode 位（闭集内）
    pub mode: u32,
    /// f_op 白名单
    pub caps: FopCaps,
    /// 生命周期状态
    pub state: HandleState,
    /// 本句柄累计放行次数
    pub ops_done: u32,
}

impl FileCtx {
    /// 装配：闭集校验 → 非空校验 → 状态 Assembled。
    pub fn assemble(mode: u32, caps: FopCaps) -> Result<FileCtx, u16> {
        if (mode & !FMODE_VALID) != 0 {
            return Err(E_MODE_UNKNOWN);
        }
        if mode == 0 {
            return Err(E_MODE_EMPTY);
        }
        Ok(FileCtx {
            mode,
            caps,
            state: HandleState::Assembled,
            ops_done: 0,
        })
    }

    /// 由 open(2) 的 flags 推出 f_mode（只取 O_ACCMODE 两位）。
    pub fn mode_from_flags(flags: u32) -> Result<u32, u16> {
        let acc = flags & O_ACCMODE;
        match acc {
            O_RDONLY => Ok(FMODE_READ | FMODE_LSEEK),
            O_WRONLY => Ok(FMODE_WRITE | FMODE_LSEEK),
            O_RDWR => Ok(FMODE_READ | FMODE_WRITE | FMODE_LSEEK),
            _ => Err(E_MODE_UNKNOWN),
        }
    }

    pub fn activate(&mut self) -> Result<(), u16> {
        match self.state {
            HandleState::Released => Err(E_RELEASED),
            HandleState::Active => Err(E_REASSEMB),
            HandleState::Assembled => {
                self.state = HandleState::Active;
                Ok(())
            }
        }
    }

    pub fn release(&mut self) -> Result<(), u16> {
        match self.state {
            HandleState::Released => Err(E_REASSEMB),
            _ => {
                self.state = HandleState::Released;
                Ok(())
            }
        }
    }

    /// 授权判定（纯函数，不入账）：门 1 f_mode，门 2 f_op。
    pub fn judge(&self, op: FileOp) -> Result<(), u16> {
        if (self.mode & op.need_mode()) == 0 {
            return Err(E_BADF);
        }
        if !self.caps.has(op) {
            return Err(E_FOP_NONE);
        }
        Ok(())
    }
}

/// 判定闸门：三账 + 矩阵导出。所有查询经此入账，账目不可绕过。
pub struct ModeGate {
    /// 放行账
    pub allow: u32,
    /// EBADF 拒收账
    pub deny: u32,
    /// 缺实现账
    pub unimpl: u32,
    /// 查询总数
    pub queries: u32,
}

impl ModeGate {
    pub fn new() -> ModeGate {
        ModeGate {
            allow: 0,
            deny: 0,
            unimpl: 0,
            queries: 0,
        }
    }

    /// 入账式判定：错误路径同样记账，不提前返回而不记。
    pub fn check(&mut self, f: &FileCtx, op: FileOp) -> Result<(), u16> {
        if f.state == HandleState::Released {
            return Err(E_RELEASED);
        }
        self.queries += 1;
        match f.judge(op) {
            Ok(()) => {
                self.allow += 1;
                Ok(())
            }
            Err(E_BADF) => {
                self.deny += 1;
                Err(E_BADF)
            }
            Err(E_FOP_NONE) => {
                self.unimpl += 1;
                Err(E_FOP_NONE)
            }
            Err(e) => Err(e),
        }
    }

    /// 三账守恒：allow + deny + unimpl == queries。
    pub fn conserved(&self) -> bool {
        self.allow + self.deny + self.unimpl == self.queries
    }

    /// 约束矩阵导出：4 种模式 × 6 个操作 = 24 格，每格给出诊断码（0 表放行）。
    /// 第 4 行是「读 + 执行」组合（O_PATH/execve 通路），覆盖只读+执行语义。
    pub fn export_matrix(caps: FopCaps) -> Vec<(u32, u8, u16)> {
        let modes = [O_RDONLY, O_WRONLY, O_RDWR, O_RDONLY | 0o100000];
        let mut out = Vec::new();
        for &m in &modes {
            let mode = if m == (O_RDONLY | 0o100000) {
                FMODE_READ | FMODE_EXEC | FMODE_LSEEK
            } else {
                match FileCtx::mode_from_flags(m) {
                    Ok(v) => v,
                    Err(_) => 0,
                }
            };
            let ctx = FileCtx {
                mode,
                caps,
                state: HandleState::Active,
                ops_done: 0,
            };
            let mut code: u8 = 0;
            while (code as usize) < OP_COUNT {
                if let Some(op) = FileOp::from_code(code) {
                    let verdict = match ctx.judge(op) {
                        Ok(()) => 0u16,
                        Err(e) => e,
                    };
                    out.push((m, code, verdict));
                }
                code += 1;
            }
        }
        out
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BADF => "f_mode 未授予该操作（EBADF 9）",
        E_MODE_EMPTY => "打开模式为空，无任何访问位",
        E_MODE_UNKNOWN => "f_mode 出现闭集外位",
        E_FOP_NONE => "f_op 白名单未提供该操作",
        E_OP_UNKNOWN => "操作码不在 6 项闭集内",
        E_ACCT => "三账与查询总数不平（查询灭失）",
        E_REASSEMB => "句柄重复装配或重复释放",
        E_RELEASED => "已释放句柄再次查询",
        _ => "未知诊断码",
    }
}

/// 确定性演练：正常路径 → 边界 → 注入拦截 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：只读打开调 write 注入 10/10 次全部 EBADF（判据原文主断言）。
    let mut gate = ModeGate::new();
    let mut ro = FileCtx::assemble(
        FileCtx::mode_from_flags(O_RDONLY)?,
        FopCaps::regular_file(),
    )?;
    ro.activate()?;
    let mut badf: u32 = 0;
    let mut i = 0;
    while i < 10 {
        if gate.check(&ro, FileOp::Write) == Err(E_BADF) {
            badf += 1;
        }
        i += 1;
    }
    if badf != 10 || gate.deny != 10 {
        return Err(E_BADF);
    }
    hits += 1;

    // R2：同句柄 read 放行 10/10 次（对偶面）。
    let mut ok_read: u32 = 0;
    i = 0;
    while i < 10 {
        if gate.check(&ro, FileOp::Read).is_ok() {
            ok_read += 1;
        }
        i += 1;
    }
    if ok_read != 10 || gate.allow != 10 {
        return Err(E_BADF);
    }
    hits += 1;

    // R3：只写打开调 read 注入 10/10 次全部 EBADF。
    let mut wo = FileCtx::assemble(
        FileCtx::mode_from_flags(O_WRONLY)?,
        FopCaps::regular_file(),
    )?;
    wo.activate()?;
    let before = gate.deny;
    i = 0;
    while i < 10 {
        if gate.check(&wo, FileOp::Read) != Err(E_BADF) {
            return Err(E_BADF);
        }
        i += 1;
    }
    if gate.deny - before != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：O_RDWR 双通路 + exec/lseek 放行，allow 累加到 14。
    let mut rw = FileCtx::assemble(
        FileCtx::mode_from_flags(O_RDWR)?,
        FopCaps::regular_file(),
    )?;
    rw.activate()?;
    gate.check(&rw, FileOp::Read)?;
    gate.check(&rw, FileOp::Write)?;
    gate.check(&rw, FileOp::Exec)?;
    gate.check(&rw, FileOp::Lseek)?;
    if gate.allow != 14 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：矩阵导出 4×6=24 格；只读行的 write 格必为 EBADF。
    let m = ModeGate::export_matrix(FopCaps::regular_file());
    if m.len() != 24 {
        return Err(E_ACCT);
    }
    let mut cells = 0;
    for &(mode, op, verdict) in &m {
        if mode == O_RDONLY && FileOp::from_code(op) == Some(FileOp::Write) {
            if verdict != E_BADF {
                return Err(E_BADF);
            }
            cells += 1;
        }
        if verdict != 0 && verdict != E_BADF && verdict != E_FOP_NONE {
            return Err(E_ACCT); // 矩阵只允许这三种落点
        }
    }
    if cells != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R6：写-only 句柄 exec 必拒（执行只认读权限）。
    if gate.check(&wo, FileOp::Exec) != Err(E_BADF) {
        return Err(E_BADF);
    }
    hits += 1;

    // I1：模式为空 → E_MODE_EMPTY（与闭集校验可区分）。
    match FileCtx::assemble(0, FopCaps::regular_file()) {
        Err(E_MODE_EMPTY) => {}
        _ => return Err(E_MODE_EMPTY),
    }
    hits += 1;

    // I2：闭集外位 → E_MODE_UNKNOWN。
    if FileCtx::assemble(FMODE_READ | 0x0000_8000, FopCaps::regular_file()).is_ok() {
        return Err(E_MODE_UNKNOWN);
    }
    hits += 1;

    // I3：只读设备缺 write 能力 → E_FOP_NONE（门 2 生效，与 EBADF 可区分）。
    let mut dev = FileCtx::assemble(
        FileCtx::mode_from_flags(O_RDWR)?,
        FopCaps::read_only_dev(),
    )?;
    dev.activate()?;
    if gate.check(&dev, FileOp::Write) != Err(E_FOP_NONE) {
        return Err(E_FOP_NONE);
    }
    if gate.unimpl != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I4：操作码越界 → 闭集拒收（不给近似值）。
    if FileOp::from_code(OP_COUNT as u8).is_some() || FileOp::from_code(255).is_some() {
        return Err(E_OP_UNKNOWN);
    }
    hits += 1;

    // I5：释放后查询 → E_RELEASED，且不污染三账。
    let q_before = gate.queries;
    ro.release()?;
    if gate.check(&ro, FileOp::Read) != Err(E_RELEASED) {
        return Err(E_RELEASED);
    }
    if gate.queries != q_before {
        return Err(E_ACCT);
    }
    hits += 1;

    // I6：重复释放 → E_REASSEMB。
    if ro.release() != Err(E_REASSEMB) {
        return Err(E_REASSEMB);
    }
    hits += 1;

    // C1：三账守恒 + 查询总数精确对账（10+10+10+4+1+1=36）。
    if !gate.conserved() {
        return Err(E_ACCT);
    }
    if gate.queries != 36 || gate.allow + gate.deny + gate.unimpl != 36 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
