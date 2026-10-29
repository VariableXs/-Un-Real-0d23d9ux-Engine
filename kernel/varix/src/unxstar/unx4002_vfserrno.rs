//! unx4002_vfserrno — UNX-F4002 VFS 错误码全集与 errno 投影基座（诊断段 0x8300）。
//!
//! 判据原文：
//!   J1 VFS 全域 32 类错误码枚举与 errno 数值投影表逐项比对一致（32/32），
//!      全表比对用例一次通过。
//!   J2 投影函数对枚举外值返回 EINVAL；反向投影未命中同样返回 EINVAL。
//!   J3 闭集内 32 条错误码数值互异且语义注释齐全，闭集外数值一律拒绝入账。
//!   J4 投影账三账（正向/反向/拒绝）与总请求账守恒，可导出核对。
//! 设计纪律：
//!   1. 数值锚定 POSIX.1-2017 <errno.h>：EPERM=1 起步至 ERANGE=34，中间
//!      15/16 交界的信号空洞（15=SIGKILL 保留区）与 26（历史 EWOULDBLOCK
//!      别名区）为保留值，不进闭集、投影恒拒。
//!   2. 枚举→数值为编译期写死常量表逐项投影；数值→枚举为线性查表，
//!      未命中返回 EINVAL(22)。双向均为纯函数，无隐藏状态。
//!   3. 投影账守恒式：正向账 + 反向账 + 拒绝账 == 总请求账；任何 API
//!      调用必落其中一账，账目不灭失。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8300;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TAB_LEN: u16 = SEG_BASE + 1; // 全集表长度不等于 32（表损坏）
const E_INVAL: u16 = SEG_BASE + 2;   // 枚举外/闭集外值投影被拒
const E_CODE_GAP: u16 = SEG_BASE + 3; // 表内出现保留数值（15/26 区）
const E_CODE_DUP: u16 = SEG_BASE + 4; // 表内数值重复（唯一性失守）
const E_BAL: u16 = SEG_BASE + 5;     // 三账与总请求账不平（账目灭失）
const E_CMP_MISS: u16 = SEG_BASE + 6; // 全表比对 32/32 缺项

/// VFS 全域 32 类错误码枚举（数值按 POSIX.1-2017 锚定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Errno32 {
    EPERM = 1,   // 操作不被许可
    ENOENT = 2,  // 文件或目录不存在
    ESRCH = 3,   // 进程不存在
    EINTR = 4,   // 系统调用被信号打断
    EIO = 5,     // I/O 错误
    ENXIO = 6,   // 设备不存在或地址越界
    E2BIG = 7,   // 参数表过长
    ENOEXEC = 8, // 可执行文件格式非法
    EBADF = 9,   // 文件描述符非法
    ECHILD = 10, // 无子进程
    EAGAIN = 11, // 资源暂不可用，宜重试
    ENOMEM = 12, // 内存不足
    EACCES = 13, // 权限拒绝
    EFAULT = 14, // 用户空间地址非法
    EBUSY = 16,  // 资源忙（15 为信号保留区，跳过）
    EEXIST = 17, // 文件已存在
    EXDEV = 18,  // 跨设备链接非法
    ENODEV = 19, // 设备不存在
    ENOTDIR = 20, // 路径分量不是目录
    EISDIR = 21, // 对目录做文件操作
    EINVAL = 22, // 参数非法
    ENFILE = 23, // 系统级文件表满
    EMFILE = 24, // 进程级描述符满
    ENOTTY = 25, // 非终端设备
    EFBIG = 27,  // 文件过大（26 为历史别名保留区，跳过）
    ENOSPC = 28, // 设备无空间
    ESPIPE = 29, // 管道不可定位
    EROFS = 30,  // 只读文件系统
    EMLINK = 31, // 硬链接数超上限
    EPIPE = 32,  // 管道破裂
    EDOM = 33,   // 数学参数超出定义域
    ERANGE = 34, // 结果不可表示
}

/// 保留数值区（不进闭集）：15 为 SIGKILL 信号区、26 为历史别名区。
const RESERVED: [u16; 2] = [15, 26];

pub const FULL_SET: usize = 32;

/// 32 项全集常量表：(errno 数值, 助记名)。数值与枚举判别值逐项锚定。
pub const ERRNO_TAB: [(u16, &'static str); FULL_SET] = [
    (1, "EPERM"), (2, "ENOENT"), (3, "ESRCH"), (4, "EINTR"),
    (5, "EIO"), (6, "ENXIO"), (7, "E2BIG"), (8, "ENOEXEC"),
    (9, "EBADF"), (10, "ECHILD"), (11, "EAGAIN"), (12, "ENOMEM"),
    (13, "EACCES"), (14, "EFAULT"), (16, "EBUSY"), (17, "EEXIST"),
    (18, "EXDEV"), (19, "ENODEV"), (20, "ENOTDIR"), (21, "EISDIR"),
    (22, "EINVAL"), (23, "ENFILE"), (24, "EMFILE"), (25, "ENOTTY"),
    (27, "EFBIG"), (28, "ENOSPC"), (29, "ESPIPE"), (30, "EROFS"),
    (31, "EMLINK"), (32, "EPIPE"), (33, "EDOM"), (34, "ERANGE"),
];

/// 正向投影：枚举 → errno 数值（纯函数，判别值即数值）。
pub fn project(e: Errno32) -> u16 {
    e as u16
}

/// 反向投影：errno 数值 → 枚举，闭集外返回 Err(E_INVAL)（判据 J2）。
pub fn reverse(code: u16) -> Result<Errno32, u16> {
    for (c, _) in ERRNO_TAB.iter() {
        if *c == code {
            // 判别值连续映射：从数值恢复枚举，逐档比对定位。
            return match code {
                1 => Ok(Errno32::EPERM), 2 => Ok(Errno32::ENOENT),
                3 => Ok(Errno32::ESRCH), 4 => Ok(Errno32::EINTR),
                5 => Ok(Errno32::EIO), 6 => Ok(Errno32::ENXIO),
                7 => Ok(Errno32::E2BIG), 8 => Ok(Errno32::ENOEXEC),
                9 => Ok(Errno32::EBADF), 10 => Ok(Errno32::ECHILD),
                11 => Ok(Errno32::EAGAIN), 12 => Ok(Errno32::ENOMEM),
                13 => Ok(Errno32::EACCES), 14 => Ok(Errno32::EFAULT),
                16 => Ok(Errno32::EBUSY), 17 => Ok(Errno32::EEXIST),
                18 => Ok(Errno32::EXDEV), 19 => Ok(Errno32::ENODEV),
                20 => Ok(Errno32::ENOTDIR), 21 => Ok(Errno32::EISDIR),
                22 => Ok(Errno32::EINVAL), 23 => Ok(Errno32::ENFILE),
                24 => Ok(Errno32::EMFILE), 25 => Ok(Errno32::ENOTTY),
                27 => Ok(Errno32::EFBIG), 28 => Ok(Errno32::ENOSPC),
                29 => Ok(Errno32::ESPIPE), 30 => Ok(Errno32::EROFS),
                31 => Ok(Errno32::EMLINK), 32 => Ok(Errno32::EPIPE),
                33 => Ok(Errno32::EDOM), _ => Ok(Errno32::ERANGE),
            };
        }
    }
    Err(E_INVAL)
}

/// 投影基座状态机：Unloaded → Comparing → Verified。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TablePhase {
    Unloaded = 0,  // 未做全表比对
    Comparing = 1, // 比对进行中
    Verified = 2,  // 32/32 已过，可对外投影
}

/// 单条投影对：数值 + 助记名 + 反向命中标记（导出面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProjectionPair {
    pub code: u16,           // errno 数值
    pub name: &'static str,  // 助记名
    pub reversed: bool,      // 反向投影是否命中
}

/// 投影基座：全表对账状态 + 三账计数账。
pub struct ErrnoBase {
    pub cmp_done: bool,     // 全表 32/32 比对是否已通过
    pub cmp_hits: u32,      // 全表比对通过次数
    pub fwd_count: u32,     // 正向投影账
    pub rev_count: u32,     // 反向命中账
    pub rej_count: u32,     // 拒绝账（闭集外）
    pub req_total: u32,     // 总请求账
    pub phase: TablePhase,  // 基座状态机
}

impl ErrnoBase {
    pub fn new() -> ErrnoBase {
        ErrnoBase {
            cmp_done: false,
            cmp_hits: 0,
            fwd_count: 0,
            rev_count: 0,
            rej_count: 0,
            req_total: 0,
            phase: TablePhase::Unloaded,
        }
    }

    /// 状态机读出口。
    pub fn phase(&self) -> TablePhase {
        self.phase
    }

    /// 单条投影对导出：闭集外返回 E_INVAL（判据 J2 的单条面）。
    pub fn probe(&self, code: u16) -> Result<ProjectionPair, u16> {
        for (c, n) in ERRNO_TAB.iter() {
            if *c == code {
                return Ok(ProjectionPair { code: *c, name: n, reversed: true });
            }
        }
        Err(E_INVAL)
    }

    /// 全表比对：32 项逐项验证枚举判别值 == 表值，并验证表完整性。
    /// 返回通过项数（应为 32），任何缺项/错位返回 E_CMP_MISS。
    pub fn full_table_compare(&mut self) -> Result<u32, u16> {
        self.phase = TablePhase::Comparing;
        if ERRNO_TAB.len() != FULL_SET {
            return Err(E_TAB_LEN);
        }
        let all = [
            Errno32::EPERM, Errno32::ENOENT, Errno32::ESRCH, Errno32::EINTR,
            Errno32::EIO, Errno32::ENXIO, Errno32::E2BIG, Errno32::ENOEXEC,
            Errno32::EBADF, Errno32::ECHILD, Errno32::EAGAIN, Errno32::ENOMEM,
            Errno32::EACCES, Errno32::EFAULT, Errno32::EBUSY, Errno32::EEXIST,
            Errno32::EXDEV, Errno32::ENODEV, Errno32::ENOTDIR, Errno32::EISDIR,
            Errno32::EINVAL, Errno32::ENFILE, Errno32::EMFILE, Errno32::ENOTTY,
            Errno32::EFBIG, Errno32::ENOSPC, Errno32::ESPIPE, Errno32::EROFS,
            Errno32::EMLINK, Errno32::EPIPE, Errno32::EDOM, Errno32::ERANGE,
        ];
        if all.len() != FULL_SET {
            return Err(E_TAB_LEN);
        }
        let mut ok = 0u32;
        for e in all.iter() {
            let v = project(*e);
            let mut found = false;
            for (c, _) in ERRNO_TAB.iter() {
                if *c == v {
                    found = true;
                    break;
                }
            }
            if !found {
                return Err(E_CMP_MISS);
            }
            ok += 1;
        }
        // 保留区数值不得入表（判据 J3 前半）。
        for r in RESERVED.iter() {
            for (c, _) in ERRNO_TAB.iter() {
                if c == r {
                    return Err(E_CODE_GAP);
                }
            }
        }
        // 数值唯一性（判据 J3 后半）。
        for a in 0..FULL_SET {
            for b in (a + 1)..FULL_SET {
                if ERRNO_TAB[a].0 == ERRNO_TAB[b].0 {
                    return Err(E_CODE_DUP);
                }
            }
        }
        self.cmp_done = true;
        self.cmp_hits += 1;
        self.phase = TablePhase::Verified;
        Ok(ok)
    }

    /// 带账正向投影：入正向账与总请求账。
    pub fn project_logged(&mut self, e: Errno32) -> Result<u16, u16> {
        self.fwd_count += 1;
        self.req_total += 1;
        Ok(project(e))
    }

    /// 带账反向投影：命中入反向账，未命中入拒绝账（总请求账均 +1）。
    pub fn reverse_logged(&mut self, code: u16) -> Result<Errno32, u16> {
        self.req_total += 1;
        match reverse(code) {
            Ok(e) => {
                self.rev_count += 1;
                Ok(e)
            }
            Err(c) => {
                self.rej_count += 1;
                Err(c)
            }
        }
    }

    /// 守恒核对：正向账 + 反向账 + 拒绝账 == 总请求账（判据 J4）。
    pub fn conserve(&self) -> bool {
        self.fwd_count + self.rev_count + self.rej_count == self.req_total
    }

    /// 助记名导出：闭集内返回表内名，闭集外 None。
    pub fn name_of(&self, code: u16) -> Option<&'static str> {
        for (c, n) in ERRNO_TAB.iter() {
            if *c == code {
                return Some(n);
            }
        }
        None
    }

    /// 闭集成员判定。
    pub fn in_set(&self, code: u16) -> bool {
        self.name_of(code).is_some()
    }

    /// 保留区判定：15/26 属信号与历史保留区。
    pub fn is_reserved(&self, code: u16) -> bool {
        code == RESERVED[0] || code == RESERVED[1]
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TAB_LEN => "全集表长度损坏（不等于 32）",
        E_INVAL => "枚举外/闭集外值投影被拒（EINVAL 路径）",
        E_CODE_GAP => "表内出现保留数值（15 信号区/26 历史区）",
        E_CODE_DUP => "表内 errno 数值重复（唯一性失守）",
        E_BAL => "投影三账与总请求账不平",
        E_CMP_MISS => "全表 32/32 比对缺项",
        _ => "未知诊断码",
    }
}

/// 确定性演练：全表比对 → 双向往返 → 枚举外注入拦截 → 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut base = ErrnoBase::new();

    // R1：全表 32/32 比对一次通过（判据 J1 原文）。
    let ok = base.full_table_compare()?;
    if ok != 32 || !base.cmp_done || base.phase() != TablePhase::Verified {
        return Err(E_CMP_MISS);
    }
    hits += 1;

    // R2：正向投影带账——EPERM=1、EINVAL=22、ERANGE=34 三锚点抽验。
    if base.project_logged(Errno32::EPERM)? != 1 {
        return Err(E_INVAL);
    }
    if base.project_logged(Errno32::EINVAL)? != 22 {
        return Err(E_INVAL);
    }
    if base.project_logged(Errno32::ERANGE)? != 34 {
        return Err(E_INVAL);
    }
    hits += 1;

    // R3：反向投影双向往返——闭集内全部 32 项 project→reverse 还原。
    for (c, _) in ERRNO_TAB.iter() {
        let e = base.reverse_logged(*c)?;
        if project(e) != *c {
            return Err(E_INVAL);
        }
    }
    if base.rev_count != 32 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：枚举外值注入——0/15/26/35/999 全部返回 EINVAL 且入拒绝账。
    for bad in [0u16, 15, 26, 35, 999].iter() {
        if reverse(*bad) != Err(E_INVAL) {
            return Err(E_INVAL);
        }
        if base.reverse_logged(*bad) != Err(E_INVAL) {
            return Err(E_INVAL);
        }
        if base.is_reserved(*bad) && !(*bad == 15 || *bad == 26) {
            return Err(E_CODE_GAP);
        }
    }
    if base.rej_count != 5 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：助记名导出——EINVAL 命中、保留区 15 未命中。
    let pair = base.probe(22)?;
    if pair.name != "EINVAL" || pair.code != 22 || !pair.reversed {
        return Err(E_CMP_MISS);
    }
    if base.probe(15).err() != Some(E_INVAL) {
        return Err(E_CODE_GAP);
    }
    if base.name_of(22) != Some("EINVAL") {
        return Err(E_CMP_MISS);
    }
    if base.name_of(15).is_some() {
        return Err(E_CODE_GAP);
    }
    if !base.in_set(30) || base.in_set(0) {
        return Err(E_CMP_MISS);
    }
    hits += 1;

    // R6：守恒对账——fwd 3 + rev 32 + rej 5 == req 40（判据 J4）。
    if !base.conserve() {
        return Err(E_BAL);
    }
    if base.fwd_count != 3 || base.req_total != 40 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：投影函数对闭集外数值恒拒（重复注入确认拦截稳定）。
    if base.reverse_logged(7) != Ok(Errno32::E2BIG) {
        return Err(E_INVAL);
    }
    if base.reverse_logged(9999) != Err(E_INVAL) {
        return Err(E_INVAL);
    }
    hits += 1;

    Ok(hits)
}
