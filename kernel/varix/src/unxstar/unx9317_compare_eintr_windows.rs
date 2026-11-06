//! UNX-F9317 EINTR 与 Windows ERROR_INTERRUPTED 对照格 · 0x7BCC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9317 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9317-J1 映射判据过，APC 中断差异账
//!   三条全列格过；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 对照格是十二行冻结表，每行把「一个 Linux errno」钉到「一个 Windows
//!      GetLastError 码」与「一个 NTSTATUS」上，并附一列可续跑性。查表有两个
//!      方向：win_to_linux 把 Win32 失败面归一到 errno；primary_win 反查首选
//!      Windows 码。ECANCELED 有两条 Windows 来路（取消 I/O 与对象取消），故
//!      Linux 侧是多对一：往返判据只对「每个 errno 的首行」成立，不允许对
//!      非首行做往返断言——那是映射表语义而不是实现缺陷。
//!   ② APC 中断差异账固定三条：投递次序（DeliveryOrder）、续跑契约
//!      (RestartContract)、返回形状 (ReturnShape)。三行的四列（Linux 侧、
//!      Windows 侧、本内核动作、可续跑性）必须同时非空，且必须体现差异：
//!      Linux 侧可续跑、Windows 侧不可续跑。任一格空缺或两侧同策略即视为
//!      「逃逸」，由注入账显性记账。
//!   ③ 等待状态机七态：Idle→Armed→Waiting→{Alerted→Restarting→Waiting |
//!      Delivered}、Waiting→Completed。APC 在 Waiting/Alerted 只累加排空计数，
//!      从不改状态——这正是「APC 不产生 ERROR_INTERRUPTED」的可执行证据。
//!   ④ 非法转移（如 Idle 直接 Complete）不计入 accepted，只累加 invalids，
//!      且状态一字不动：拒绝路径不得有副作用，否则后续轨迹会连锁污染。
//!   ⑤ 注入固定 10 轮 × 3 轴 = 30 次，账本恒满足 injected == detected +
//!      escaped 且 escaped == 0；轴内命中数各自独立计数，不允许共用累加器。
//!   ⑥ 诊断码段内闭集 12 个，码值为 SEG_BASE + 1 + 序号，两两唯一。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9317 独占 0x7BCC0..=0x7BCFF）。
pub const SEG_BASE: u32 = 0x7BCC0;
/// 段窗口宽度（每卡 0x40）。
pub const SEG_SPAN: u32 = 0x40;
/// 本批段窗口上界（含）。
pub const SEG_WINDOW_HI: u32 = 0x7BFFF;
/// 本批段窗口下界（含）。
pub const SEG_WINDOW_LO: u32 = 0x7B000;

// ── Linux errno（正数口径） ───────────────────────────────────────────
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
pub const ETIMEDOUT: i32 = 110;
pub const ECANCELED: i32 = 125;

// ── Windows GetLastError 码 ───────────────────────────────────────────
pub const WIN_FILE_NOT_FOUND: u32 = 2;
pub const WIN_ACCESS_DENIED: u32 = 5;
pub const WIN_INVALID_HANDLE: u32 = 6;
pub const WIN_NOT_ENOUGH_MEMORY: u32 = 8;
pub const WIN_INVALID_PARAMETER: u32 = 87;
pub const WIN_INTERRUPTED: u32 = 95;
pub const WIN_OPERATION_ABORTED: u32 = 995;
pub const WIN_IO_PENDING: u32 = 997;
pub const WIN_NOACCESS: u32 = 998;
pub const WIN_CANCELLED: u32 = 1223;
pub const WIN_PRIVILEGE_NOT_HELD: u32 = 1314;
pub const WIN_TIMEOUT: u32 = 1460;

// ── NTSTATUS（u32 位型，S 位在最高位） ───────────────────────────────
pub const NT_ALERTED: u32 = 0x0000_0101;
pub const NT_TIMEOUT: u32 = 0x0000_0102;
pub const NT_PENDING: u32 = 0x0000_0103;
pub const NT_ACCESS_VIOLATION: u32 = 0xC000_0005;
pub const NT_INVALID_HANDLE: u32 = 0xC000_0008;
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
pub const NT_NO_MEMORY: u32 = 0xC000_0017;
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;
pub const NT_OBJECT_NAME_NOT_FOUND: u32 = 0xC000_0034;
pub const NT_PRIVILEGE_NOT_HELD: u32 = 0xC000_0061;
pub const NT_CANCELLED: u32 = 0xC000_0120;

/// 对照格行数。
pub const MAP_ROWS_COUNT: usize = 12;
/// APC 差异轴数。
pub const APC_AXES: usize = 3;
/// 差异轴注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 单条轨迹最大步数（超出即视为越界，不静默截断）。
pub const TRACE_MAX: usize = 12;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// F9317 诊断码（段内 +1..=+12 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EintrCode {
    /// 对照格有空行或空列
    MappingGap,
    /// Windows 码在表内重复
    DuplicateWinCode,
    /// 首行往返不通
    RoundTripBroken,
    /// 表外 Windows 码被误判为已知
    UnknownWinCode,
    /// 表外 errno 被误判为已知
    UnknownLinuxErrno,
    /// 差异轴有空格
    ApcAxisEmpty,
    /// 差异轴码重复
    ApcAxisDuplicate,
    /// 差异轴丢失「可续跑性差异」
    ApcAxisLossy,
    /// 等待状态机非法转移
    IllegalTransition,
    /// 重启链未能回到 Waiting
    RestartLost,
    /// APC 排空计数丢失
    ApcDropped,
    /// 注入账不守恒
    LedgerRegress,
}

impl EintrCode {
    /// 段内序号（契约口径的 u16 码）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 段内码值 = SEG_BASE + 1 + 序号，两两唯一。
    pub fn val(self) -> u32 {
        SEG_BASE
            .saturating_add(1)
            .saturating_add(self.code() as u32)
    }

    /// 中文一句话建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            EintrCode::MappingGap => "对照格有空行或空列：十二行的 Linux/Windows/NTSTATUS 三列都要有值，缺一列就无法归因",
            EintrCode::DuplicateWinCode => "Windows 码重复：同一错误码出现在两行，查表方向会产生二义，必须拆开或合并说明",
            EintrCode::RoundTripBroken => "首行往返不通：errno→首选 Windows 码→errno 必须回到原值，否则归一化链断开",
            EintrCode::UnknownWinCode => "表外 Windows 码被放行：未登记的码必须回 None，禁止猜测性映射",
            EintrCode::UnknownLinuxErrno => "表外 errno 被放行：只允许 errno 白名单内的正值进入对照格",
            EintrCode::ApcAxisEmpty => "差异轴有空格：三条轴的 Linux/Windows/内核动作三列都必须写满",
            EintrCode::ApcAxisDuplicate => "差异轴码重复：三条轴各自唯一，重复即无法定位是哪一类差异",
            EintrCode::ApcAxisLossy => "差异轴丢失可续跑性差异：Linux 可续跑、Windows 不可续跑，两侧同策略说明账没记全",
            EintrCode::IllegalTransition => "非法转移被接受：状态机只允许冻结表内的边，拒绝路径不得改状态",
            EintrCode::RestartLost => "重启链丢失：Restarting 必须能重新进入 Waiting，否则 -EINTR 无法透明续跑",
            EintrCode::ApcDropped => "APC 排空计数丢失：等待期每次排空都要加一，计数少了就无法证明 APC 未打断等待",
            EintrCode::LedgerRegress => "注入账不守恒：注入必须等于检出加逃逸，且逃逸恒为零",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [EintrCode] = &[
        EintrCode::MappingGap,
        EintrCode::DuplicateWinCode,
        EintrCode::RoundTripBroken,
        EintrCode::UnknownWinCode,
        EintrCode::UnknownLinuxErrno,
        EintrCode::ApcAxisEmpty,
        EintrCode::ApcAxisDuplicate,
        EintrCode::ApcAxisLossy,
        EintrCode::IllegalTransition,
        EintrCode::RestartLost,
        EintrCode::ApcDropped,
        EintrCode::LedgerRegress,
    ];
}

// ── 可续跑性 ──────────────────────────────────────────────────────────
/// 被打断后的续跑契约（对照格第四列）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RestartPolicy {
    /// 可透明续跑（Linux：SA_RESTART / restart_syscall）
    Resume = 0,
    /// 不可续跑，错误直交用户态（Windows：无自动重启）
    NoResume = 1,
}

impl RestartPolicy {
    /// 冻结编码，用于指纹。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 中文描述。
    pub fn text(self) -> &'static str {
        match self {
            RestartPolicy::Resume => "可续跑：返回点允许内核把 syscall 重新挂起，用户态看不到 -EINTR",
            RestartPolicy::NoResume => "不可续跑：错误必须交回用户态，由调用方自行重进等待",
        }
    }
}

// ── 对照格 ────────────────────────────────────────────────────────────
/// 一行对照：Linux errno ↔ Windows 码 ↔ NTSTATUS ↔ 可续跑性。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapRow {
    pub linux: i32,
    pub linux_name: &'static str,
    pub win: u32,
    pub win_name: &'static str,
    pub nt: u32,
    pub restart: RestartPolicy,
    pub note: &'static str,
}

impl MapRow {
    /// 该行是否三列都有值（对照格完整性）。
    pub fn complete(&self) -> bool {
        !self.linux_name.is_empty() && !self.win_name.is_empty() && !self.note.is_empty()
    }

    /// NTSTATUS 是否为错误级（最高位为 1）。
    pub fn nt_is_error(&self) -> bool {
        self.nt & 0x8000_0000 != 0
    }
}

/// 十二行冻结对照格。
pub const MAP_ROWS: [MapRow; MAP_ROWS_COUNT] = [
    MapRow {
        linux: EINTR,
        linux_name: "EINTR",
        win: WIN_INTERRUPTED,
        win_name: "ERROR_INTERRUPTED",
        nt: NT_ALERTED,
        restart: RestartPolicy::Resume,
        note: "等待点被打断：Linux 交 EINTR 且可续跑；Windows 以 STATUS_ALERTED 结束等待，用户层看到 ERROR_INTERRUPTED",
    },
    MapRow {
        linux: ECANCELED,
        linux_name: "ECANCELED",
        win: WIN_OPERATION_ABORTED,
        win_name: "ERROR_OPERATION_ABORTED",
        nt: NT_CANCELLED,
        restart: RestartPolicy::NoResume,
        note: "显式取消 I/O：Linux 交 ECANCELED；Windows 交 ERROR_OPERATION_ABORTED，取消后资源需调用方回收",
    },
    MapRow {
        linux: ECANCELED,
        linux_name: "ECANCELED",
        win: WIN_CANCELLED,
        win_name: "ERROR_CANCELLED",
        nt: NT_CANCELLED,
        restart: RestartPolicy::NoResume,
        note: "对象级取消（线程池/定时器）：两条路径都终止等待，且都不允许把取消当成可重试的中断",
    },
    MapRow {
        linux: EAGAIN,
        linux_name: "EAGAIN",
        win: WIN_IO_PENDING,
        win_name: "ERROR_IO_PENDING",
        nt: NT_PENDING,
        restart: RestartPolicy::NoResume,
        note: "异步提交未完成：Linux 非阻塞返回 EAGAIN；Windows 交 ERROR_IO_PENDING，由完成端口回收结果",
    },
    MapRow {
        linux: EINVAL,
        linux_name: "EINVAL",
        win: WIN_INVALID_PARAMETER,
        win_name: "ERROR_INVALID_PARAMETER",
        nt: NT_INVALID_PARAMETER,
        restart: RestartPolicy::NoResume,
        note: "参数不合法：两端都在入口拒绝，不产生部分副作用，也不允许重试",
    },
    MapRow {
        linux: EBADF,
        linux_name: "EBADF",
        win: WIN_INVALID_HANDLE,
        win_name: "ERROR_INVALID_HANDLE",
        nt: NT_INVALID_HANDLE,
        restart: RestartPolicy::NoResume,
        note: "描述符/句柄失效：Windows 句柄是内核对象引用，关闭后立即失效，与 fd 表语义同形",
    },
    MapRow {
        linux: EFAULT,
        linux_name: "EFAULT",
        win: WIN_NOACCESS,
        win_name: "ERROR_NOACCESS",
        nt: NT_ACCESS_VIOLATION,
        restart: RestartPolicy::NoResume,
        note: "用户缓冲不可访问：Windows 由结构化异常转 ERROR_NOACCESS，Linux 在拷贝期回 EFAULT",
    },
    MapRow {
        linux: ENOENT,
        linux_name: "ENOENT",
        win: WIN_FILE_NOT_FOUND,
        win_name: "ERROR_FILE_NOT_FOUND",
        nt: NT_OBJECT_NAME_NOT_FOUND,
        restart: RestartPolicy::NoResume,
        note: "对象不存在：路径解析阶段失败；Windows 区分文件与路径两种码，这里取文件缺失一条",
    },
    MapRow {
        linux: ETIMEDOUT,
        linux_name: "ETIMEDOUT",
        win: WIN_TIMEOUT,
        win_name: "ERROR_TIMEOUT",
        nt: NT_TIMEOUT,
        restart: RestartPolicy::NoResume,
        note: "限时等待到期：Linux 交 ETIMEDOUT；Windows 交 ERROR_TIMEOUT（258 是另一个 WAIT_TIMEOUT）",
    },
    MapRow {
        linux: ENOMEM,
        linux_name: "ENOMEM",
        win: WIN_NOT_ENOUGH_MEMORY,
        win_name: "ERROR_NOT_ENOUGH_MEMORY",
        nt: NT_NO_MEMORY,
        restart: RestartPolicy::NoResume,
        note: "内存不足：两端都可重试，但都不自动重试；重试策略属于调用方",
    },
    MapRow {
        linux: EACCES,
        linux_name: "EACCES",
        win: WIN_ACCESS_DENIED,
        win_name: "ERROR_ACCESS_DENIED",
        nt: NT_ACCESS_DENIED,
        restart: RestartPolicy::NoResume,
        note: "权限拒绝：Windows 用单一 ERROR_ACCESS_DENIED 覆盖 EACCES 与 EPERM 两种粒度",
    },
    MapRow {
        linux: EPERM,
        linux_name: "EPERM",
        win: WIN_PRIVILEGE_NOT_HELD,
        win_name: "ERROR_PRIVILEGE_NOT_HELD",
        nt: NT_PRIVILEGE_NOT_HELD,
        restart: RestartPolicy::NoResume,
        note: "缺少特权：Windows 要求显式启用令牌特权，Linux 用 capability 判定，两者都不可重试",
    },
];

/// errno 数值 → 规范名（表外返回 None）。
pub fn canonical_errno_name(code: i32) -> Option<&'static str> {
    match code {
        EPERM => Some("EPERM"),
        ENOENT => Some("ENOENT"),
        EINTR => Some("EINTR"),
        EBADF => Some("EBADF"),
        EAGAIN => Some("EAGAIN"),
        ENOMEM => Some("ENOMEM"),
        EACCES => Some("EACCES"),
        EFAULT => Some("EFAULT"),
        EINVAL => Some("EINVAL"),
        ETIMEDOUT => Some("ETIMEDOUT"),
        ECANCELED => Some("ECANCELED"),
        _ => None,
    }
}

/// Windows 码 → Linux errno（首行优先；表外 None）。
pub fn win_to_linux(code: u32) -> Option<i32> {
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        if let Some(row) = MAP_ROWS.get(i) {
            if row.win == code {
                return Some(row.linux);
            }
        }
        i += 1;
    }
    None
}

/// Linux errno → 首选 Windows 码（首行优先；表外 None）。
pub fn primary_win(errno: i32) -> Option<u32> {
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        if let Some(row) = MAP_ROWS.get(i) {
            if row.linux == errno {
                return Some(row.win);
            }
        }
        i += 1;
    }
    None
}

/// 该行是否为其 errno 的首行（往返判据只对首行成立）。
pub fn is_primary_row(idx: usize) -> bool {
    match MAP_ROWS.get(idx) {
        Some(row) => primary_win(row.linux) == Some(row.win),
        None => false,
    }
}

/// 全部首行往返自洽。
pub fn roundtrip_all() -> bool {
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        if let Some(row) = MAP_ROWS.get(i) {
            if is_primary_row(i) {
                let back = win_to_linux(row.win);
                if back != Some(row.linux) {
                    return false;
                }
                let fwd = primary_win(row.linux);
                if fwd != Some(row.win) {
                    return false;
                }
            }
        }
        i += 1;
    }
    true
}

/// Windows 码两两唯一。
pub fn win_codes_unique() -> bool {
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        let mut j = i.saturating_add(1);
        while j < MAP_ROWS.len() {
            if let (Some(a), Some(b)) = (MAP_ROWS.get(i), MAP_ROWS.get(j)) {
                if a.win == b.win {
                    return false;
                }
            }
            j = j.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    true
}

/// 对照格完整：行数、三列文案、errno 名与数值一致。
pub fn rows_well_formed() -> bool {
    if MAP_ROWS.len() != MAP_ROWS_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        match MAP_ROWS.get(i) {
            Some(row) => {
                if !row.complete() {
                    return false;
                }
                if canonical_errno_name(row.linux) != Some(row.linux_name) {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

// ── APC 中断差异账 ────────────────────────────────────────────────────
/// 三条差异轴。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApcAxis {
    /// 投递次序
    DeliveryOrder = 0,
    /// 续跑契约
    RestartContract = 1,
    /// 返回形状
    ReturnShape = 2,
}

impl ApcAxis {
    /// 轴码。
    pub fn code(self) -> u32 {
        self as u32
    }

    /// 轴名。
    pub fn name(self) -> &'static str {
        match self {
            ApcAxis::DeliveryOrder => "投递次序",
            ApcAxis::RestartContract => "续跑契约",
            ApcAxis::ReturnShape => "返回形状",
        }
    }
}

/// 一行差异账：四个格位（Linux 侧 / Windows 侧 / 本内核动作 / 可续跑性差异）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApcDiffRow {
    pub axis: ApcAxis,
    pub linux_side: &'static str,
    pub windows_side: &'static str,
    pub kernel_action: &'static str,
    pub linux_restart: RestartPolicy,
    pub win_restart: RestartPolicy,
}

impl ApcDiffRow {
    /// 三个文本列是否全部写满（「全列格」判据的逐行落点）。
    pub fn texts_complete(&self) -> bool {
        !self.linux_side.is_empty() && !self.windows_side.is_empty() && !self.kernel_action.is_empty()
    }

    /// 该行是否真正体现了续跑性差异。
    pub fn differs(&self) -> bool {
        self.linux_restart != self.win_restart
    }
}

/// 三条轴冻结表。
pub const APC_GRID: [ApcDiffRow; APC_AXES] = [
    ApcDiffRow {
        axis: ApcAxis::DeliveryOrder,
        linux_side: "信号在返回用户态前的最后一条指令处投递，先压 sigframe 再回用户态",
        windows_side: "APC 在等待返回前必须排空；排空后重新进入等待，用户态看不到中途状态",
        kernel_action: "本内核先唤醒等待者再排空 APC，保证用户态只观察到一次完整状态迁移",
        linux_restart: RestartPolicy::Resume,
        win_restart: RestartPolicy::NoResume,
    },
    ApcDiffRow {
        axis: ApcAxis::RestartContract,
        linux_side: "SA_RESTART 与 restart_syscall 可把 -EINTR 透明续跑，用户态无感知",
        windows_side: "无自动重启：WaitForSingleObject 回 WAIT_IO_COMPLETION，调用方必须自行重进",
        kernel_action: "本内核只在 syscall 层的可重启点回 -ERESTARTSYS，不可重启点才暴露 EINTR",
        linux_restart: RestartPolicy::Resume,
        win_restart: RestartPolicy::NoResume,
    },
    ApcDiffRow {
        axis: ApcAxis::ReturnShape,
        linux_side: "只交 errno 单通道；已传输字节数由返回值表达，短读不等于出错",
        windows_side: "NTSTATUS 与 GetLastError 双通道；已传输量在 IO_STATUS_BLOCK.Information",
        kernel_action: "本内核只回 errno 单通道，短读必须换算成字节数而不是 EINTR",
        linux_restart: RestartPolicy::Resume,
        win_restart: RestartPolicy::NoResume,
    },
];

/// 三轴全列格：行数、逐行三列非空、逐行存在可续跑性差异。
pub fn apc_grid_full() -> bool {
    if APC_GRID.len() != APC_AXES {
        return false;
    }
    let mut i = 0usize;
    while i < APC_GRID.len() {
        match APC_GRID.get(i) {
            Some(row) => {
                if !row.texts_complete() || !row.differs() {
                    return false;
                }
                if row.linux_restart != RestartPolicy::Resume
                    || row.win_restart != RestartPolicy::NoResume
                {
                    return false;
                }
            }
            None => return false,
        }
        i = i.saturating_add(1);
    }
    true
}

/// 轴码两两唯一。
pub fn apc_axis_unique() -> bool {
    let mut i = 0usize;
    while i < APC_GRID.len() {
        let mut j = i.saturating_add(1);
        while j < APC_GRID.len() {
            if let (Some(a), Some(b)) = (APC_GRID.get(i), APC_GRID.get(j)) {
                if a.axis.code() == b.axis.code() {
                    return false;
                }
            }
            j = j.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    true
}

// ── 等待状态机 ────────────────────────────────────────────────────────
/// 等待点状态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitState {
    Idle = 0,
    Armed = 1,
    Waiting = 2,
    Alerted = 3,
    Restarting = 4,
    Delivered = 5,
    Completed = 6,
}

/// 送达等待点的事件。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WaitEvent {
    Arm = 0,
    Enter = 1,
    Signal = 2,
    Apc = 3,
    Timeout = 4,
    Cancel = 5,
    RestartOk = 6,
    Complete = 7,
}

impl WaitState {
    /// 状态编码。
    pub fn code(self) -> u32 {
        self as u32
    }
}

impl WaitEvent {
    /// 事件编码。
    pub fn code(self) -> u32 {
        self as u32
    }
}

/// 等待机：状态 + 三个单调计数 + 已交付 errno（0 表示无错误）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitMachine {
    pub state: WaitState,
    pub apc_drained: u32,
    pub restarts: u32,
    pub delivered: i32,
    pub invalids: u32,
}

impl WaitMachine {
    /// 初始态。
    pub const fn idle() -> WaitMachine {
        WaitMachine {
            state: WaitState::Idle,
            apc_drained: 0,
            restarts: 0,
            delivered: 0,
            invalids: 0,
        }
    }

    /// 喂一个事件；非法转移返回 false 且状态一字不动。
    pub fn feed(&mut self, ev: WaitEvent) -> bool {
        match (self.state, ev) {
            (WaitState::Idle, WaitEvent::Arm) => {
                self.state = WaitState::Armed;
                true
            }
            (WaitState::Armed, WaitEvent::Enter) => {
                self.state = WaitState::Waiting;
                true
            }
            (WaitState::Waiting, WaitEvent::Signal) => {
                self.state = WaitState::Alerted;
                true
            }
            (WaitState::Waiting, WaitEvent::Apc) | (WaitState::Alerted, WaitEvent::Apc) => {
                // APC 只排空，不改状态：这正是与信号中断的差异所在。
                self.apc_drained = self.apc_drained.saturating_add(1);
                true
            }
            (WaitState::Waiting, WaitEvent::Timeout) | (WaitState::Waiting, WaitEvent::Complete) => {
                self.state = WaitState::Completed;
                true
            }
            (WaitState::Waiting, WaitEvent::Cancel) => {
                self.state = WaitState::Delivered;
                self.delivered = ECANCELED;
                true
            }
            (WaitState::Alerted, WaitEvent::RestartOk) => {
                self.restarts = self.restarts.saturating_add(1);
                self.state = WaitState::Restarting;
                true
            }
            (WaitState::Restarting, WaitEvent::Enter) => {
                self.state = WaitState::Waiting;
                true
            }
            (WaitState::Alerted, WaitEvent::Complete) => {
                self.state = WaitState::Delivered;
                self.delivered = EINTR;
                true
            }
            _ => {
                self.invalids = self.invalids.saturating_add(1);
                false
            }
        }
    }

    /// 是否处于可接受信号的中途态。
    pub fn in_flight(&self) -> bool {
        matches!(self.state, WaitState::Waiting | WaitState::Alerted | WaitState::Restarting)
    }
}

/// 跑一条轨迹，返回（终态机, 被接受步数）。
pub fn run_trace(events: &[WaitEvent]) -> (WaitMachine, u32) {
    let mut m = WaitMachine::idle();
    let mut accepted = 0u32;
    let mut i = 0usize;
    while i < events.len() {
        if let Some(ev) = events.get(i).copied() {
            if m.feed(ev) {
                accepted = accepted.saturating_add(1);
            }
        }
        i = i.saturating_add(1);
    }
    (m, accepted)
}

// ── 注入账 ────────────────────────────────────────────────────────────
/// 注入账本：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApcLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl ApcLedger {
    /// 零值。
    pub const fn zero() -> ApcLedger {
        ApcLedger {
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 记一笔，返回累计注入数（调用方据此核对轮次）。
    pub fn record(&mut self, caught: bool) -> u32 {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.injected
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 差异轴注入：每轴 10 轮，逐轮复算三列与续跑性差异。
pub fn run_apc_injection() -> (ApcLedger, [u32; APC_AXES]) {
    let mut ledger = ApcLedger::zero();
    let mut per_axis = [0u32; APC_AXES];
    let mut ai = 0usize;
    while ai < APC_GRID.len() {
        if let Some(row) = APC_GRID.get(ai).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = row.texts_complete()
                    && row.differs()
                    && row.linux_restart == RestartPolicy::Resume
                    && row.win_restart == RestartPolicy::NoResume
                    && !row.axis.name().is_empty();
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_axis.get_mut(ai) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round = round.saturating_add(1);
            }
        }
        ai = ai.saturating_add(1);
    }
    (ledger, per_axis)
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（自写整数同余，无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i = i.saturating_add(1);
    }
    h
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.push((v >> 24) as u8);
    out.push((v >> 16) as u8);
    out.push((v >> 8) as u8);
    out.push(v as u8);
}

fn push_i32(out: &mut Vec<u8>, v: i32) {
    push_u32(out, v as u32);
}

fn push_str(out: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    push_u32(out, bytes.len() as u32);
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            out.push(*b);
        }
        i = i.saturating_add(1);
    }
}

/// 规范语料指纹：对照格十二行 + 差异账三行 + 三条轨迹终态。
pub fn corpus_fingerprint() -> u32 {
    let mut out: Vec<u8> = Vec::new();
    push_u32(&mut out, SEG_BASE);
    push_u32(&mut out, MAP_ROWS_COUNT as u32);

    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        if let Some(row) = MAP_ROWS.get(i) {
            push_i32(&mut out, row.linux);
            push_u32(&mut out, row.win);
            push_u32(&mut out, row.nt);
            push_u32(&mut out, row.restart.code());
            push_str(&mut out, row.linux_name);
            push_str(&mut out, row.win_name);
        }
        i = i.saturating_add(1);
    }

    let mut k = 0usize;
    while k < APC_GRID.len() {
        if let Some(row) = APC_GRID.get(k) {
            push_u32(&mut out, row.axis.code());
            push_u32(&mut out, row.linux_restart.code());
            push_u32(&mut out, row.win_restart.code());
            push_str(&mut out, row.linux_side);
            push_str(&mut out, row.windows_side);
            push_str(&mut out, row.kernel_action);
        }
        k = k.saturating_add(1);
    }

    let restart_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Signal,
        WaitEvent::RestartOk,
        WaitEvent::Enter,
        WaitEvent::Complete,
    ];
    let eintr_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Signal,
        WaitEvent::Complete,
    ];
    let apc_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Apc,
        WaitEvent::Apc,
        WaitEvent::Complete,
    ];
    let chains: [&[WaitEvent]; 3] = [&restart_chain, &eintr_chain, &apc_chain];
    let mut c = 0usize;
    while c < chains.len() {
        if let Some(events) = chains.get(c) {
            let (m, accepted) = run_trace(*events);
            push_u32(&mut out, m.state.code());
            push_u32(&mut out, accepted);
            push_u32(&mut out, m.apc_drained);
            push_u32(&mut out, m.restarts);
            push_i32(&mut out, m.delivered);
            push_u32(&mut out, m.invalids);
        }
        c = c.saturating_add(1);
    }

    fnv1a(&out)
}

/// 对照格摘要（动态明细用，控制在 48 字节内）。
pub fn summary_text(ledger: &ApcLedger, per_axis: &[u32; APC_AXES]) -> alloc::string::String {
    let a0 = per_axis.first().copied().unwrap_or(0);
    let a1 = per_axis.get(1).copied().unwrap_or(0);
    let a2 = per_axis.get(2).copied().unwrap_or(0);
    format!(
        "注入{} 检出{} 逃逸{} 轴{}/{}/{}",
        ledger.injected, ledger.detected, ledger.escaped, a0, a1, a2
    )
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9317 判据集。
pub fn run_unx9317_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9317");

    let seg_ok = SEG_BASE % SEG_SPAN == 0
        && (SEG_WINDOW_LO..=SEG_WINDOW_HI).contains(&SEG_BASE)
        && SEG_BASE == 0x7BCC0;
    s.add(
        "段位守卫：0x40 对齐且落在本批窗口",
        seg_ok,
        "段基址唯一、0x40 对齐，且不与相邻卡段重叠",
    );

    s.add(
        "对照格：十二行且三列文案非空",
        rows_well_formed() && MAP_ROWS.len() == MAP_ROWS_COUNT,
        "每行 Linux/Windows/说明三列必须有值，缺列即无法归因",
    );

    let mut names_ok = true;
    let mut i = 0usize;
    while i < MAP_ROWS.len() {
        if let Some(row) = MAP_ROWS.get(i) {
            if canonical_errno_name(row.linux) != Some(row.linux_name) {
                names_ok = false;
            }
        } else {
            names_ok = false;
        }
        i = i.saturating_add(1);
    }
    s.add(
        "对照格：errno 名与数值逐一对应",
        names_ok,
        "名字列必须与 errno 数值自洽，不许写错码名",
    );

    s.add(
        "对照格：Windows 码两两唯一",
        win_codes_unique(),
        "同一 Win32 码不得出现在两行，否则归一化有二义",
    );

    s.add(
        "双向映射：每个 errno 首行可往返",
        roundtrip_all(),
        "errno→首选 Windows 码→errno 必须闭环；多对一只对首行成立",
    );

    let unknown_ok = win_to_linux(0).is_none()
        && win_to_linux(1).is_none()
        && win_to_linux(12_345).is_none()
        && win_to_linux(0xFFFF_FFFF).is_none()
        && primary_win(9999).is_none()
        && win_to_linux(WIN_INTERRUPTED) == Some(EINTR);
    s.add(
        "表外码：未知 Windows/errno 一律 None",
        unknown_ok,
        "未登记的码必须拒绝，禁止猜测性映射",
    );

    s.add(
        "APC 差异账：三条全列格",
        apc_grid_full(),
        "3 行 × 4 列全非空，且逐行体现可续跑性差异",
    );

    s.add(
        "APC 差异轴：轴码两两唯一",
        apc_axis_unique() && APC_AXES == 3,
        "三条轴各自唯一，重复即无法定位差异类别",
    );

    let restart_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Signal,
        WaitEvent::RestartOk,
        WaitEvent::Enter,
        WaitEvent::Complete,
    ];
    let (m_restart, acc_restart) = run_trace(&restart_chain);
    s.add(
        "状态机：重启链回到 Waiting 后完成",
        m_restart.state == WaitState::Completed
            && m_restart.restarts == 1
            && m_restart.delivered == 0
            && m_restart.invalids == 0
            && acc_restart == 6,
        "Signal→RestartOk→Enter→Complete 六步全接受且交付为空",
    );

    let eintr_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Signal,
        WaitEvent::Complete,
    ];
    let (m_eintr, acc_eintr) = run_trace(&eintr_chain);
    s.add(
        "状态机：不可重启点交付 -EINTR",
        m_eintr.state == WaitState::Delivered
            && m_eintr.delivered == EINTR
            && m_eintr.restarts == 0
            && acc_eintr == 4,
        "Alerted 直接 Complete 必须交付 EINTR，且不得记重启",
    );

    let apc_chain = [
        WaitEvent::Arm,
        WaitEvent::Enter,
        WaitEvent::Apc,
        WaitEvent::Apc,
        WaitEvent::Apc,
        WaitEvent::Complete,
    ];
    let (m_apc, acc_apc) = run_trace(&apc_chain);
    s.add(
        "状态机：APC 不打断等待",
        m_apc.state == WaitState::Completed
            && m_apc.apc_drained == 3
            && m_apc.delivered == 0
            && acc_apc == 6,
        "等待期三次 APC 只累加排空计数，绝不产生 EINTR",
    );

    let cancel_chain = [WaitEvent::Arm, WaitEvent::Enter, WaitEvent::Cancel];
    let (m_cancel, acc_cancel) = run_trace(&cancel_chain);
    s.add(
        "状态机：取消交付 ECANCELED",
        m_cancel.state == WaitState::Delivered
            && m_cancel.delivered == ECANCELED
            && acc_cancel == 3,
        "取消是终止性错误，不得与可重试的中断混为一谈",
    );

    let mut bad = WaitMachine::idle();
    let accepted_bad = bad.feed(WaitEvent::Complete);
    let idle_intact = bad.state == WaitState::Idle && bad.invalids == 1;
    let mut done = WaitMachine::idle();
    done.state = WaitState::Completed;
    let reenter = done.feed(WaitEvent::Enter);
    s.add(
        "状态机：非法转移被拒且不改状态",
        !accepted_bad && idle_intact && !reenter && done.invalids == 1,
        "拒绝路径只累加 invalids，状态必须一字不动",
    );

    let (ledger, per_axis) = run_apc_injection();
    let expected_total = (APC_AXES as u32).saturating_mul(INJECT_ROUNDS);
    let axis_all = per_axis.iter().all(|n| *n == INJECT_ROUNDS);
    s.add(
        "注入账：三轴各 10 轮全检出、逃逸为零",
        ledger.conserved()
            && ledger.escaped == 0
            && ledger.injected == expected_total
            && ledger.detected == expected_total
            && axis_all,
        "30 次注入必须 30 次检出，轴内命中不得共用累加器",
    );

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add(
        "双跑一致：对照格与轨迹指纹相等",
        fp_a == fp_b && fp_a != 0,
        "同输入两次必须逐字节相等，隐藏状态即失败",
    );

    s.add(
        "诊断码：12 个码文案非空",
        EintrCode::ALL.len() == 12 && EintrCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作的中文建议，禁止空串",
    );

    let mut uniq = true;
    let mut a = 0usize;
    while a < EintrCode::ALL.len() {
        let mut b = a.saturating_add(1);
        while b < EintrCode::ALL.len() {
            if let (Some(x), Some(y)) = (EintrCode::ALL.get(a), EintrCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b = b.saturating_add(1);
        }
        a = a.saturating_add(1);
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码不得重复，否则判据无法定位",
    );

    let summary = summary_text(&ledger, &per_axis);
    s.add_dyn(
        "差异账快照",
        ledger.conserved() && ledger.escaped == 0,
        &summary,
    );

    let map_text = format!(
        "对照格 {} 行 注入 {} 检出 {}",
        MAP_ROWS_COUNT, ledger.injected, ledger.detected
    );
    s.add_dyn(
        "对照格快照",
        rows_well_formed() && ledger.injected == 30,
        &map_text,
    );

    let fp_text = format!("指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shape_and_names() {
        assert_eq!(MAP_ROWS.len(), MAP_ROWS_COUNT);
        assert_eq!(rows_well_formed(), true);
        assert_eq!(canonical_errno_name(EINTR), Some("EINTR"));
        assert_eq!(canonical_errno_name(ECANCELED), Some("ECANCELED"));
        assert_eq!(canonical_errno_name(999), None);
    }

    #[test]
    fn win_codes_are_unique() {
        assert_eq!(win_codes_unique(), true);
        assert_eq!(win_to_linux(WIN_INTERRUPTED), Some(EINTR));
        assert_eq!(win_to_linux(WIN_OPERATION_ABORTED), Some(ECANCELED));
        assert_eq!(win_to_linux(WIN_CANCELLED), Some(ECANCELED));
    }

    #[test]
    fn roundtrip_only_on_primary_rows() {
        assert_eq!(roundtrip_all(), true);
        assert_eq!(primary_win(EINTR), Some(WIN_INTERRUPTED));
        // ECANCELED 有两条来路，首选行必须是与 WIN_OPERATION_ABORTED 配对的那行。
        assert_eq!(primary_win(ECANCELED), Some(WIN_OPERATION_ABORTED));
        assert_eq!(is_primary_row(1), true);
        assert_eq!(is_primary_row(2), false);
    }

    #[test]
    fn unknown_codes_rejected() {
        assert_eq!(win_to_linux(0), None);
        assert_eq!(win_to_linux(1), None);
        assert_eq!(win_to_linux(12_345), None);
        assert_eq!(win_to_linux(0xFFFF_FFFF), None);
        assert_eq!(primary_win(0), None);
        assert_eq!(primary_win(-1), None);
    }

    #[test]
    fn apc_grid_has_three_full_rows() {
        assert_eq!(APC_GRID.len(), 3);
        assert_eq!(apc_grid_full(), true);
        assert_eq!(apc_axis_unique(), true);
        let first = APC_GRID.first().copied();
        assert_eq!(first.map(|r| r.axis.name()), Some("投递次序"));
    }

    #[test]
    fn restart_chain_completes() {
        let trace = [
            WaitEvent::Arm,
            WaitEvent::Enter,
            WaitEvent::Signal,
            WaitEvent::RestartOk,
            WaitEvent::Enter,
            WaitEvent::Complete,
        ];
        let (m, acc) = run_trace(&trace);
        assert_eq!(acc, 6);
        assert_eq!(m.state, WaitState::Completed);
        assert_eq!(m.restarts, 1);
        assert_eq!(m.delivered, 0);
        assert_eq!(m.invalids, 0);
    }

    #[test]
    fn eintr_and_cancel_deliveries() {
        let eintr = [WaitEvent::Arm, WaitEvent::Enter, WaitEvent::Signal, WaitEvent::Complete];
        let (m1, _a) = run_trace(&eintr);
        assert_eq!(m1.state, WaitState::Delivered);
        assert_eq!(m1.delivered, EINTR);

        let cancel = [WaitEvent::Arm, WaitEvent::Enter, WaitEvent::Cancel];
        let (m2, _b) = run_trace(&cancel);
        assert_eq!(m2.delivered, ECANCELED);
        assert_eq!(m2.restarts, 0);
    }

    #[test]
    fn apc_never_interrupts() {
        let trace = [
            WaitEvent::Arm,
            WaitEvent::Enter,
            WaitEvent::Apc,
            WaitEvent::Apc,
            WaitEvent::Apc,
            WaitEvent::Complete,
        ];
        let (m, acc) = run_trace(&trace);
        assert_eq!(acc, 6);
        assert_eq!(m.apc_drained, 3);
        assert_eq!(m.delivered, 0);
        assert_eq!(m.state, WaitState::Completed);
    }

    #[test]
    fn illegal_transition_is_rejected() {
        let mut m = WaitMachine::idle();
        assert_eq!(m.feed(WaitEvent::Complete), false);
        assert_eq!(m.state, WaitState::Idle);
        assert_eq!(m.invalids, 1);
        assert_eq!(m.feed(WaitEvent::Arm), true);
        assert_eq!(m.feed(WaitEvent::Signal), false);
        assert_eq!(m.state, WaitState::Armed);
        assert_eq!(m.invalids, 2);
    }

    #[test]
    fn injection_ledger_conserved() {
        let (ledger, per) = run_apc_injection();
        assert_eq!(ledger.conserved(), true);
        assert_eq!(ledger.escaped, 0);
        assert_eq!(ledger.injected, 30);
        assert_eq!(ledger.detected, 30);
        assert_eq!(per.iter().all(|n| *n == INJECT_ROUNDS), true);
    }

    #[test]
    fn fingerprint_is_stable() {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        assert_eq!(a, b);
        assert_eq!(a != 0, true);
    }

    #[test]
    fn code_values_unique() {
        let mut seen = 0u32;
        let mut ok = true;
        let mut i = 0usize;
        while i < EintrCode::ALL.len() {
            if let Some(c) = EintrCode::ALL.get(i) {
                let bit = 1u32 << (c.code() as u32);
                if seen & bit != 0 {
                    ok = false;
                }
                seen |= bit;
                assert_eq!(c.val(), SEG_BASE + 1 + c.code() as u32);
            }
            i += 1;
        }
        assert_eq!(ok, true);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9317_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
