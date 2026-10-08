//! AI-03 · F055 — 错误码 ABI.
//!
//! One table, two views. The kernel names each error with an `Errno` variant;
//! the user side sees the *Linux-compatible* negative number so a compat shim
//! (F070) can pass it through untouched. `sysret_*`/`sysret_decode` are the
//! only sanctioned encodings — a raw `-1` never leaves this module.
//!
//! The whole point is that the mapping is total and reversible: every variant
//! has exactly one number, every number maps back to exactly one variant, and
//! a self-test walks the table to prove it (no silent drift between kernel and
//! user semantics).

/// Canonical Varix error set (F055).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Errno {
    /// Not an error — used only inside the kernel; never returned as a failure.
    Ok = 0,
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
}

impl Errno {
    /// Linux-compatible negative value handed to user space (F055/F070).
    pub const fn as_linux(self) -> i32 {
        match self {
            Errno::Ok => 0,
            Errno::Perm => -1,
            Errno::NoEnt => -2,
            Errno::Intr => -4,
            Errno::Io => -5,
            Errno::Again => -11,
            Errno::NoMem => -12,
            Errno::Fault => -14,
            Errno::Busy => -16,
            Errno::Exists => -17,
            Errno::NoDev => -19,
            Errno::NotDir => -20,
            Errno::IsDir => -21,
            Errno::Inval => -22,
            Errno::NoBufs => -105,
            Errno::NoSpc => -28,
            Errno::BadF => -9,
            Errno::NoSys => -38,
            Errno::Pipe => -32,
            Errno::Range => -34,
            Errno::Overflow => -75,
            Errno::NotSup => -95,
            Errno::Proto => -71,
            Errno::NoMsg => -42,
            Errno::TimedOut => -110,
            Errno::Child => -10,
            Errno::Xdev => -18,
            Errno::TooManyFds => -24,
            Errno::AddrFamily => -97,
            Errno::AddrInUse => -98,
            Errno::AddrNotAvail => -99,
            Errno::NetDown => -100,
            Errno::NameTooLong => -36,
            Errno::Loop => -40,
            Errno::BadMsg => -90,
            Errno::NoProtoOpt => -92,
            Errno::OwnerDead => -130,
            Errno::Stale => -116,
            Errno::Empty => -61,
            // Varix-native extensions reuse the vendor bands so the compat
            // layer can recognise them as non-Linux without a lookup.
            Errno::Quota => -1080,
            Errno::Seccomp => -1081,
            Errno::Audit => -1082,
            Errno::BusyNs => -1083,
            Errno::Denied => -1084,
        }
    }

    /// Inverse of [`Errno::as_linux`]. `None` for a value we never emit.
    pub const fn from_linux(v: i32) -> Option<Errno> {
        let e = match v {
            0 => Errno::Ok,
            -1 => Errno::Perm,
            -2 => Errno::NoEnt,
            -4 => Errno::Intr,
            -5 => Errno::Io,
            -9 => Errno::BadF,
            -10 => Errno::Child,
            -11 => Errno::Again,
            -12 => Errno::NoMem,
            -14 => Errno::Fault,
            -16 => Errno::Busy,
            -17 => Errno::Exists,
            -18 => Errno::Xdev,
            -19 => Errno::NoDev,
            -20 => Errno::NotDir,
            -21 => Errno::IsDir,
            -22 => Errno::Inval,
            -24 => Errno::TooManyFds,
            -28 => Errno::NoSpc,
            -32 => Errno::Pipe,
            -34 => Errno::Range,
            -36 => Errno::NameTooLong,
            -38 => Errno::NoSys,
            -40 => Errno::Loop,
            -42 => Errno::NoMsg,
            -61 => Errno::Empty,
            -71 => Errno::Proto,
            -75 => Errno::Overflow,
            -90 => Errno::BadMsg,
            -92 => Errno::NoProtoOpt,
            -95 => Errno::NotSup,
            -97 => Errno::AddrFamily,
            -98 => Errno::AddrInUse,
            -99 => Errno::AddrNotAvail,
            -100 => Errno::NetDown,
            -105 => Errno::NoBufs,
            -110 => Errno::TimedOut,
            -116 => Errno::Stale,
            -130 => Errno::OwnerDead,
            -1080 => Errno::Quota,
            -1081 => Errno::Seccomp,
            -1082 => Errno::Audit,
            -1083 => Errno::BusyNs,
            -1084 => Errno::Denied,
            _ => return None,
        };
        Some(e)
    }

    /// Human-readable one-liner (serial log / audit trail).
    pub const fn message(self) -> &'static str {
        match self {
            Errno::Ok => "ok",
            Errno::Perm => "operation not permitted",
            Errno::NoEnt => "no such entry",
            Errno::Intr => "interrupted",
            Errno::Io => "io error",
            Errno::Again => "try again",
            Errno::NoMem => "out of memory",
            Errno::Fault => "bad address",
            Errno::Busy => "resource busy",
            Errno::Exists => "already exists",
            Errno::BadF => "bad file descriptor",
            Errno::Inval => "invalid argument",
            Errno::NoSys => "syscall not implemented",
            Errno::Range => "result out of range",
            Errno::Pipe => "broken pipe",
            Errno::NotDir => "not a directory",
            Errno::IsDir => "is a directory",
            Errno::NoSpc => "no space left",
            Errno::Overflow => "value too large",
            Errno::NotSup => "operation not supported",
            Errno::Proto => "protocol error",
            Errno::NoMsg => "no message",
            Errno::TimedOut => "timed out",
            Errno::Child => "no such child",
            Errno::NoDev => "no such device",
            Errno::Xdev => "cross-device link",
            Errno::NoBufs => "no buffer space",
            Errno::TooManyFds => "too many open handles",
            Errno::Quota => "call quota exceeded",
            Errno::Seccomp => "blocked by seccomp filter",
            Errno::Audit => "denied by audit policy",
            Errno::BusyNs => "namespace conflict",
            Errno::BadMsg => "bad message",
            Errno::NoProtoOpt => "unsupported protocol option",
            Errno::AddrFamily => "address family not supported",
            Errno::AddrInUse => "address already in use",
            Errno::AddrNotAvail => "address not available",
            Errno::NetDown => "network is down",
            Errno::NameTooLong => "name too long",
            Errno::Loop => "too many symbolic links",
            Errno::OwnerDead => "owner died",
            Errno::Stale => "stale handle",
            Errno::Empty => "queue empty",
            Errno::Denied => "denied by policy",
        }
    }

    /// Whether user space may sensibly retry the call verbatim.
    pub const fn is_retryable(self) -> bool {
        matches!(self, Errno::Intr | Errno::Again | Errno::Busy | Errno::BusyNs)
    }

    /// Errors that mean "your arguments were structurally wrong", i.e. the
    /// classes the parameter-safety layer (F054) may emit.
    pub const fn is_arg_error(self) -> bool {
        matches!(
            self,
            Errno::Fault | Errno::Inval | Errno::Range | Errno::Overflow
        )
    }
}

/// Return channel: a plain non-negative value.
pub const fn sysret_ok(v: u64) -> u64 {
    v
}

/// Return channel: an error, encoded as the two's-complement Linux value.
pub const fn sysret_err(e: Errno) -> u64 {
    e.as_linux() as i64 as u64
}

/// Decode a raw syscall return slot (F055).
pub const fn sysret_decode(raw: u64) -> Result<u64, Errno> {
    let signed = raw as i64;
    if signed < 0 && signed >= -4095 && signed <= -1 {
        match Errno::from_linux(signed as i32) {
            Some(e) => Err(if matches!(e, Errno::Ok) { Errno::Inval } else { e }),
            // An in-band negative without a table entry is a kernel bug; do not
            // let it masquerade as a huge success value.
            None => Err(Errno::Inval),
        }
    } else {
        Ok(raw)
    }
}

/// Largest value the success channel can carry. The error channel occupies
/// `[-4095, -1]`, so a success value must be representable as a positive
/// `i64` — a caller that has more than `i64::MAX` to report must split it.
pub const SYSRET_MAX_OK: u64 = i64::MAX as u64;

/// True when a raw return slot carries an error.
pub const fn sysret_is_err(raw: u64) -> bool {
    let sx = raw as i64;
    sx < 0 && sx >= -4095
}

/// The number of distinct variants, used by the self-test sweep.
pub const ERRNO_COUNT: usize = 44;

/// Full table in declaration order — walked by the F055 self-check to prove
/// the number mapping is injective.
pub const ERRNO_TABLE: [Errno; ERRNO_COUNT] = [
    Errno::Ok,
    Errno::Perm,
    Errno::NoEnt,
    Errno::Intr,
    Errno::Io,
    Errno::Again,
    Errno::NoMem,
    Errno::Fault,
    Errno::Busy,
    Errno::Exists,
    Errno::BadF,
    Errno::Inval,
    Errno::NoSys,
    Errno::Range,
    Errno::Pipe,
    Errno::NotDir,
    Errno::IsDir,
    Errno::NoSpc,
    Errno::Overflow,
    Errno::NotSup,
    Errno::Proto,
    Errno::NoMsg,
    Errno::TimedOut,
    Errno::Child,
    Errno::NoDev,
    Errno::Xdev,
    Errno::NoBufs,
    Errno::TooManyFds,
    Errno::Quota,
    Errno::Seccomp,
    Errno::Audit,
    Errno::BusyNs,
    Errno::BadMsg,
    Errno::NoProtoOpt,
    Errno::AddrFamily,
    Errno::AddrInUse,
    Errno::AddrNotAvail,
    Errno::NetDown,
    Errno::NameTooLong,
    Errno::Loop,
    Errno::OwnerDead,
    Errno::Stale,
    Errno::Empty,
    Errno::Denied,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f055_numbering_is_a_bijection() {
        assert_eq!(ERRNO_TABLE.len(), ERRNO_COUNT);
        for (i, e) in ERRNO_TABLE.iter().enumerate() {
            assert_eq!(Errno::from_linux(e.as_linux()), Some(*e), "round trip {i}");
            assert_eq!(*e as i32, i as i32, "declaration order {i}");
        }
        // Distinct numbers, no collisions.
        for i in 0..ERRNO_COUNT {
            for j in (i + 1)..ERRNO_COUNT {
                assert_ne!(
                    ERRNO_TABLE[i].as_linux(),
                    ERRNO_TABLE[j].as_linux(),
                    "collision {} vs {}",
                    i,
                    j
                );
            }
        }
    }

    #[test]
    fn f055_return_channel_round_trips() {
        assert_eq!(sysret_ok(0x1234), 0x1234);
        assert!(!sysret_is_err(sysret_ok(SYSRET_MAX_OK)));
        assert_eq!(sysret_decode(sysret_ok(SYSRET_MAX_OK)), Ok(SYSRET_MAX_OK));
        for e in ERRNO_TABLE {
            if e == Errno::Ok {
                continue;
            }
            let raw = sysret_err(e);
            assert!(sysret_is_err(raw), "{e:?} must look like an error");
            assert_eq!(sysret_decode(raw), Err(e));
        }
        // In-band negatives are classified against the table; a negative with
        // no entry is refused instead of leaking out as a huge success value.
        assert_eq!(sysret_decode(u64::MAX), Err(Errno::Perm)); // -1 就是 EPERM
        assert_eq!(sysret_decode(-4095i64 as u64), Err(Errno::Inval));
        assert!(!Errno::Perm.is_retryable());
        assert!(Errno::Again.is_retryable());
        assert!(Errno::Fault.is_arg_error());
    }
}
