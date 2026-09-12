//! F055/F058 · 错误类型与系统调用返回通道.
//!
//! 内核在 RAX 里回一个 `u64`：非负即成功，落在 `[-4095, -1]` 即错误
//! （负数按 Linux 约定编码）。本模块是**唯一**解码该通道的地方，上层永远
//! 看不到裸的负数。

use crate::syscall0_raw;

/// 与内核 `syscall::errno::Errno` 同构的用户态错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Error {
    Perm = 1,
    NoEnt = 2,
    Intr = 3,
    Io = 4,
    Again = 5,
    NoMem = 6,
    Fault = 7,
    Busy = 8,
    Exists = 9,
    BadF = 10,
    Inval = 11,
    NoSys = 12,
    Range = 13,
    Pipe = 14,
    NotDir = 15,
    IsDir = 16,
    NoSpc = 17,
    Overflow = 18,
    NotSup = 19,
    Proto = 20,
    NoMsg = 21,
    TimedOut = 22,
    Child = 23,
    NoDev = 24,
    Xdev = 25,
    NoBufs = 26,
    TooManyFds = 27,
    Quota = 28,
    Seccomp = 29,
    Audit = 30,
    BusyNs = 31,
    BadMsg = 32,
    NoProtoOpt = 33,
    AddrFamily = 34,
    AddrInUse = 35,
    AddrNotAvail = 36,
    NetDown = 37,
    NameTooLong = 38,
    Loop = 39,
    OwnerDead = 40,
    Stale = 41,
    Empty = 42,
    Denied = 43,
    /// 内核回了一个表里没有的负数：宁可报未知错误，也不要当成成功。
    Unknown = 4095,
}

impl Error {
    /// 从内核原始返回值解码（F055）。
    pub const fn from_raw(raw: u64) -> core::result::Result<u64, Error> {
        let sx = raw as i64;
        if sx >= 0 {
            return Ok(raw);
        }
        if sx < -4095 {
            return Err(Error::Unknown);
        }
        Err(match sx {
            -1 => Error::Perm,
            -2 => Error::NoEnt,
            -4 => Error::Intr,
            -5 => Error::Io,
            -9 => Error::BadF,
            -10 => Error::Child,
            -11 => Error::Again,
            -12 => Error::NoMem,
            -14 => Error::Fault,
            -16 => Error::Busy,
            -17 => Error::Exists,
            -18 => Error::Xdev,
            -19 => Error::NoDev,
            -20 => Error::NotDir,
            -21 => Error::IsDir,
            -22 => Error::Inval,
            -24 => Error::TooManyFds,
            -28 => Error::NoSpc,
            -32 => Error::Pipe,
            -34 => Error::Range,
            -36 => Error::NameTooLong,
            -38 => Error::NoSys,
            -40 => Error::Loop,
            -42 => Error::NoMsg,
            -61 => Error::Empty,
            -71 => Error::Proto,
            -75 => Error::Overflow,
            -90 => Error::BadMsg,
            -92 => Error::NoProtoOpt,
            -95 => Error::NotSup,
            -97 => Error::AddrFamily,
            -98 => Error::AddrInUse,
            -99 => Error::AddrNotAvail,
            -100 => Error::NetDown,
            -105 => Error::NoBufs,
            -110 => Error::TimedOut,
            -116 => Error::Stale,
            -130 => Error::OwnerDead,
            -1080 => Error::Quota,
            -1081 => Error::Seccomp,
            -1082 => Error::Audit,
            -1083 => Error::BusyNs,
            _ => Error::Denied,
        })
    }

    /// 内核侧错误码（取负）。
    pub const fn raw_code(self) -> i32 {
        -(self as i32)
    }

    pub const fn message(self) -> &'static str {
        match self {
            Error::Perm => "operation not permitted",
            Error::NoEnt => "no such entry",
            Error::Intr => "interrupted",
            Error::Io => "io error",
            Error::Again => "try again",
            Error::NoMem => "out of memory",
            Error::Fault => "bad address",
            Error::Busy => "resource busy",
            Error::Exists => "already exists",
            Error::BadF => "bad file descriptor",
            Error::Inval => "invalid argument",
            Error::NoSys => "syscall not implemented",
            Error::Range => "result out of range",
            Error::Pipe => "broken pipe",
            Error::NotDir => "not a directory",
            Error::IsDir => "is a directory",
            Error::NoSpc => "no space left",
            Error::Overflow => "value too large",
            Error::NotSup => "operation not supported",
            Error::Proto => "protocol error",
            Error::NoMsg => "no message",
            Error::TimedOut => "timed out",
            Error::Child => "no such child",
            Error::NoDev => "no such device",
            Error::Xdev => "cross-device link",
            Error::NoBufs => "no buffer space",
            Error::TooManyFds => "too many open handles",
            Error::Quota => "call quota exceeded",
            Error::Seccomp => "blocked by seccomp filter",
            Error::Audit => "denied by audit policy",
            Error::BusyNs => "namespace conflict",
            Error::BadMsg => "bad message",
            Error::NoProtoOpt => "unsupported protocol option",
            Error::AddrFamily => "address family not supported",
            Error::AddrInUse => "address already in use",
            Error::AddrNotAvail => "address not available",
            Error::NetDown => "network is down",
            Error::NameTooLong => "name too long",
            Error::Loop => "too many symbolic links",
            Error::OwnerDead => "owner died",
            Error::Stale => "stale handle",
            Error::Empty => "queue empty",
            Error::Denied => "denied by policy",
            Error::Unknown => "unknown kernel error",
        }
    }

    /// 直接重试同一调用是否合理（F055 语义：速率类可重试，语义类不可）。
    pub const fn is_retryable(self) -> bool {
        matches!(self, Error::Intr | Error::Again | Error::Busy | Error::BusyNs)
    }
}

/// 结果别名。
pub type Result<T> = core::result::Result<T, Error>;

/// 把 6 个参数装进 ABI 寄存器并发起 `syscall`，然后解码返回通道。
///
/// 这是本库最短的路径：不做任何参数检查——参数合法性由内核的 F054 层判定，
/// 用户态重复检查只会制造两套会漂移的规则。
#[inline(always)]
pub unsafe fn invoke(nr: u64, args: [u64; 6]) -> Result<u64> {
    let raw = unsafe { syscall0_raw(nr, args) };
    Error::from_raw(raw)
}
