//! F071 · Varix 系统调用号与 ABI 常量（与内核号表一一对应）.
//!
//! 这是**唯一**允许写死调用号的地方。号表在内核侧由 `syscall::table` 维护，
//! 内核自检会断言本文件的每一项都能在内核号表里找到（见 `syscall::userlib`）。

/// 原生号段起始（与内核 `SYS_NR_BASE` 相同）。
pub const SYS_NR_BASE: u64 = 0x4000;
/// 兼容号段起始（Linux 号经此段转接）。
pub const BAND_COMPAT_LO: u64 = 0x4100;

pub const SYS_READ: u64 = SYS_NR_BASE + 0;
pub const SYS_WRITE: u64 = SYS_NR_BASE + 1;
pub const SYS_OPEN: u64 = SYS_NR_BASE + 2;
pub const SYS_CLOSE: u64 = SYS_NR_BASE + 3;
pub const SYS_EXIT: u64 = SYS_NR_BASE + 4;
pub const SYS_WAIT: u64 = SYS_NR_BASE + 5;
pub const SYS_CLOCK: u64 = SYS_NR_BASE + 6;
pub const SYS_MMAP: u64 = SYS_NR_BASE + 7;
pub const SYS_MUNMAP: u64 = SYS_NR_BASE + 8;
pub const SYS_MPROTECT: u64 = SYS_NR_BASE + 9;
pub const SYS_EVENT_CTL: u64 = SYS_NR_BASE + 10;
pub const SYS_EVENT_WAIT: u64 = SYS_NR_BASE + 11;
pub const SYS_PIPE: u64 = SYS_NR_BASE + 12;
pub const SYS_DUP: u64 = SYS_NR_BASE + 13;
pub const SYS_SEND_HANDLE: u64 = SYS_NR_BASE + 14;
pub const SYS_ABI: u64 = SYS_NR_BASE + 15;
pub const SYS_YIELD: u64 = SYS_NR_BASE + 16;
pub const SYS_LOG: u64 = SYS_NR_BASE + 17;

/// 本库编译时对应的 ABI 版本。`AbiInfo::version_min` 必须 ≤ 本值。
pub const ABI_VERSION: u32 = 1;

// 描述符权限位（F056/F062）。
pub const RIGHT_READ: u32 = 1 << 0;
pub const RIGHT_WRITE: u32 = 1 << 1;
pub const RIGHT_TRANSFER: u32 = 1 << 2;
pub const RIGHT_ALL: u32 = RIGHT_READ | RIGHT_WRITE | RIGHT_TRANSFER;

// 标准描述符（F056）。
pub const FD_STDIN: u32 = 0;
pub const FD_STDOUT: u32 = 1;
pub const FD_STDERR: u32 = 2;

// mmap 保护位（F059）。
pub const PROT_NONE: u32 = 0;
pub const PROT_READ: u32 = 1 << 0;
pub const PROT_WRITE: u32 = 1 << 1;
pub const PROT_EXEC: u32 = 1 << 2;

// mmap 标志位（F059）。
pub const MAP_SHARED: u32 = 1 << 0;
pub const MAP_PRIVATE: u32 = 1 << 1;
pub const MAP_FIXED: u32 = 1 << 2;
pub const MAP_ANONYMOUS: u32 = 1 << 3;
pub const MAP_POPULATE: u32 = 1 << 4;
pub const MAP_STACK: u32 = 1 << 5;

// 时钟（F058）。
pub const CLOCK_REALTIME: u64 = 0;
pub const CLOCK_MONOTONIC: u64 = 1;

// wait 状态（F057）。
pub const WAIT_EXITED: u32 = 0;
pub const WAIT_SIGNALED: u32 = 1;
pub const WAIT_CORE: u32 = 1 << 7;
pub const WAIT_STOP_BIT: u32 = 0x7F;

// 事件类别（F060）。
pub const EVENT_KEY: u32 = 1;
pub const EVENT_TIMER: u32 = 2;
pub const EVENT_SIGNAL: u32 = 3;
pub const EVENT_IO: u32 = 4;
pub const EVENT_EXIT: u32 = 5;

// ABI 能力位（F069）。
pub const FEAT_CAPS: u64 = 1 << 0;
pub const FEAT_AUDIT: u64 = 1 << 1;
pub const FEAT_QUOTA: u64 = 1 << 2;
pub const FEAT_SECCOMP: u64 = 1 << 3;
pub const FEAT_VDSO: u64 = 1 << 4;
pub const FEAT_COMPAT: u64 = 1 << 5;
pub const FEAT_PIPES: u64 = 1 << 6;
pub const FEAT_EVENTPORT: u64 = 1 << 7;
pub const FEAT_HANDLE_PASSING: u64 = 1 << 8;
pub const FEAT_FASTPATH: u64 = 1 << 9;
pub const FEAT_METER: u64 = 1 << 10;
