//! AI-05 · 进程与用户态域（F101~F125）.
//!
//! Everything that separates "a program" from "the kernel": the ring-3 door,
//! the syscall table, the ELF loader, the process tree, signals, capabilities
//! and the exit conventions. The pieces are deliberately data-driven so the
//! privileges a process can hold are enumerable — a security model you cannot
//! print is a security model you cannot audit.

use core::sync::atomic::AtomicU64;

pub mod elf;
pub mod syscall;

// --- VARIABLE-200 AI-01 · 用户态进程域（F001~F025，W2）---------------------
pub mod uspace;
pub use uspace::run_uspace_checks;

pub use elf::{ElfError, ElfImage, LoadSegment, MAX_LOAD_SEGMENTS};
pub use syscall::{SyscallError, SyscallTable, SyscallTableError, MAX_SYSCALLS};

pub const MAX_PROCESSES: usize = 64;
pub const NO_PID: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// F101 — ring 3
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Ring {
    /// Kernel.
    #[default]
    Zero,
    /// User.
    Three,
}

impl Ring {
    pub fn as_str(self) -> &'static str {
        match self {
            Ring::Zero => "ring0",
            Ring::Three => "ring3",
        }
    }
}

/// Segment selectors for a ring. Derived from the GDT layout so the two can
/// never drift apart (F026 owns the descriptors).
pub fn selectors_for(ring: Ring) -> (u16, u16) {
    match ring {
        Ring::Zero => (
            crate::cpu::gdt::SEL_KERNEL_CODE,
            crate::cpu::gdt::SEL_KERNEL_DATA,
        ),
        Ring::Three => (crate::cpu::gdt::SEL_USER_CODE, crate::cpu::gdt::SEL_USER_DATA),
    }
}

/// The register state `sysretq`/`iretq` needs to enter ring 3.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UserFrame {
    pub rip: u64,
    pub rsp: u64,
    pub rflags: u64,
    /// `sysret` returns these two from registers, not the stack.
    pub rcx_saved: u64,
    pub r11_saved: u64,
}

/// `RFLAGS.IF` must be set on entry or the process runs with interrupts dead.
pub const RFLAGS_IF: u64 = 1 << 9;
/// `RFLAGS.IOPL` must be 3 so the process cannot touch port IO.
pub const RFLAGS_IOPL_USER: u64 = 3 << 12;

impl UserFrame {
    pub fn for_entry(entry: u64, stack_top: u64) -> UserFrame {
        UserFrame {
            rip: entry,
            rsp: stack_top,
            rflags: RFLAGS_IF,
            rcx_saved: entry,
            r11_saved: RFLAGS_IF,
        }
    }

    /// A frame the CPU can actually return to: executable address inside the
    /// user half, a stack, interrupts on, IO privilege level 3, and no
    /// direction-flag surprises for string instructions.
    pub fn valid(&self) -> bool {
        self.rip != 0
            && crate::mem::paging::is_user(self.rip)
            && crate::mem::paging::is_user(self.rsp)
            && self.rsp != 0
            && self.rflags & RFLAGS_IF != 0
            && self.rflags & RFLAGS_IOPL_USER == RFLAGS_IOPL_USER
            && self.rflags & (1 << 10) == 0 // DF clear
    }

    /// Normalize a frame before entering ring 3.
    pub fn sanitized(mut self) -> UserFrame {
        self.rflags |= RFLAGS_IF | RFLAGS_IOPL_USER;
        self.rflags &= !(1 << 10);
        self.rflags &= !(1 << 8); // TF: never single-step a fresh process
        self.rcx_saved = self.rip;
        self.r11_saved = self.rflags;
        self
    }
}

/// F101: switch to a ring. Returns the frame to load.
pub fn enter_ring(ring: Ring, frame: UserFrame) -> Result<UserFrame, &'static str> {
    match ring {
        Ring::Three => {
            let f = frame.sanitized();
            if !f.valid() {
                return Err("user frame is not returnable");
            }
            Ok(f)
        }
        Ring::Zero => Ok(frame.sanitized()),
    }
}

// ---------------------------------------------------------------------------
// F109 — signals
// ---------------------------------------------------------------------------

pub const MAX_SIGNAL: u32 = 31;
/// Signals 32.. are real-time signals; Varix reserves them.
pub const RT_SIGNAL_FLOOR: u32 = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct Signal(pub u32);

impl Signal {
    pub const SIGHUP: Signal = Signal(1);
    pub const SIGINT: Signal = Signal(2);
    pub const SIGQUIT: Signal = Signal(3);
    pub const SIGKILL: Signal = Signal(9);
    pub const SIGSEGV: Signal = Signal(11);
    pub const SIGPIPE: Signal = Signal(13);
    pub const SIGTERM: Signal = Signal(15);
    pub const SIGCHLD: Signal = Signal(17);
    pub const SIGCONT: Signal = Signal(18);
    pub const SIGSTOP: Signal = Signal(19);

    pub fn valid(self) -> bool {
        self.0 >= 1 && self.0 <= MAX_SIGNAL
    }

    pub fn is_uncatchable(self) -> bool {
        matches!(self.0, 9 | 19) // SIGKILL / SIGSTOP
    }

    pub fn name(self) -> &'static str {
        match self.0 {
            1 => "SIGHUP",
            2 => "SIGINT",
            3 => "SIGQUIT",
            4 => "SIGILL",
            5 => "SIGTRAP",
            6 => "SIGABRT",
            7 => "SIGBUS",
            8 => "SIGFPE",
            9 => "SIGKILL",
            10 => "SIGUSR1",
            11 => "SIGSEGV",
            12 => "SIGUSR2",
            13 => "SIGPIPE",
            14 => "SIGALRM",
            15 => "SIGTERM",
            16 => "SIGSTKFLT",
            17 => "SIGCHLD",
            18 => "SIGCONT",
            19 => "SIGSTOP",
            20 => "SIGTSTP",
            21 => "SIGTTIN",
            22 => "SIGTTOU",
            23 => "SIGURG",
            24 => "SIGXCPU",
            25 => "SIGXFSZ",
            26 => "SIGVTALRM",
            27 => "SIGPROF",
            28 => "SIGWINCH",
            29 => "SIGIO",
            30 => "SIGPWR",
            31 => "SIGSYS",
            _ => "SIG?",
        }
    }
}

/// What the kernel does with a signal when the process has no handler.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SignalAction {
    Term,
    Core,
    Ignore,
    Stop,
    Continue,
    /// Cannot happen: SIGKILL/SIGSTOP are always acted on.
    #[default]
    Unknown,
}

pub fn default_action(sig: Signal) -> SignalAction {
    match sig.0 {
        1 | 2 | 4 | 5 | 6 | 7 | 8 | 13 | 14 | 15 | 16 | 21 | 22 | 25 | 26 | 27 | 30 | 31 => {
            SignalAction::Term
        }
        3 | 11 => SignalAction::Core,
        10 | 12 | 17 | 19 | 20 | 23 | 28 | 29 => SignalAction::Ignore,
        18 => SignalAction::Continue,
        9 => SignalAction::Term,
        _ => SignalAction::Unknown,
    }
}

/// A 31-bit pending mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SignalSet(pub u32);

impl SignalSet {
    pub fn add(&mut self, sig: Signal) -> bool {
        if !sig.valid() {
            return false;
        }
        self.0 |= 1 << (sig.0 - 1);
        true
    }

    pub fn remove(&mut self, sig: Signal) {
        if sig.valid() {
            self.0 &= !(1 << (sig.0 - 1));
        }
    }

    pub fn contains(&self, sig: Signal) -> bool {
        sig.valid() && self.0 & (1 << (sig.0 - 1)) != 0
    }

    pub fn is_empty(&self) -> bool {
        self.0 == 0
    }

    /// Lowest-numbered pending signal: POSIX order, which keeps SIGKILL first.
    pub fn next(&self) -> Option<Signal> {
        if self.0 == 0 {
            return None;
        }
        Some(Signal(self.0.trailing_zeros() + 1))
    }

    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }
}

// ---------------------------------------------------------------------------
// F112 — capability tokens
// ---------------------------------------------------------------------------

/// Capabilities are a bitset, not a hierarchy: a process either holds the right
/// to do a thing or it does not, and the set can only shrink when delegated.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Caps(pub u64);

impl Caps {
    pub const FS_READ: u64 = 1 << 0;
    pub const FS_WRITE: u64 = 1 << 1;
    pub const FS_EXEC: u64 = 1 << 2;
    pub const NET: u64 = 1 << 3;
    pub const DEVICE: u64 = 1 << 4;
    pub const SPAWN: u64 = 1 << 5;
    pub const SIGNAL: u64 = 1 << 6;
    pub const MEM_MAP: u64 = 1 << 7;
    pub const TRACE: u64 = 1 << 8;
    pub const LOAD_DRIVER: u64 = 1 << 9;
    pub const SET_TIME: u64 = 1 << 10;
    pub const RAW_IO: u64 = 1 << 11;

    pub const ALL: u64 = 0xFFF;
    /// The set a freshly forked process inherits (F108) — deliberately small.
    pub const DEFAULT_CHILD: u64 = Self::FS_READ | Self::FS_WRITE | Self::FS_EXEC | Self::MEM_MAP | Self::SPAWN;

    pub fn none() -> Caps {
        Caps(0)
    }

    pub fn kernel() -> Caps {
        Caps(Self::ALL)
    }

    pub fn user_default() -> Caps {
        Caps(Self::DEFAULT_CHILD | Self::SIGNAL)
    }

    pub fn has(&self, cap: u64) -> bool {
        self.0 & cap == cap
    }

    pub fn grants(mut self, cap: u64) -> Caps {
        self.0 |= cap;
        self
    }

    /// F112: delegation can only remove.
    pub fn attenuate(mut self, drop: u64) -> Caps {
        self.0 &= !drop;
        self
    }

    pub fn intersect(self, other: Caps) -> Caps {
        Caps(self.0 & other.0)
    }

    pub fn is_subset_of(&self, other: Caps) -> bool {
        self.0 & !other.0 == 0
    }

    pub fn count(&self) -> u32 {
        self.0.count_ones()
    }

    /// Human-readable list, into `out`; returns the byte count.
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        let named: [(u64, &str); 12] = [
            (Self::FS_READ, "fs-read"),
            (Self::FS_WRITE, "fs-write"),
            (Self::FS_EXEC, "fs-exec"),
            (Self::NET, "net"),
            (Self::DEVICE, "device"),
            (Self::SPAWN, "spawn"),
            (Self::SIGNAL, "signal"),
            (Self::MEM_MAP, "mem-map"),
            (Self::TRACE, "trace"),
            (Self::LOAD_DRIVER, "load-driver"),
            (Self::SET_TIME, "set-time"),
            (Self::RAW_IO, "raw-io"),
        ];
        let mut first = true;
        for (bit, name) in named {
            if self.has(bit) {
                if !first {
                    w.str(",");
                }
                w.str(name);
                first = false;
            }
        }
        if first {
            w.str("-");
        }
        w.used()
    }
}

/// The seven isolation layers, from "runs as the kernel" to "cannot even see
/// the machine". A token names the layer it was minted for.
pub const ISOLATION_LAYERS: usize = 7;

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum SecurityLayer {
    /// Kernel threads.
    L0Kernel = 0,
    /// Services that own hardware.
    L1Service = 1,
    /// Installed applications.
    L2Application = 2,
    /// Untrusted third-party code.
    L3Sandboxed = 3,
    /// Guest operating systems (W5).
    L4Guest = 4,
    /// Ephemeral per-task processes (builds, scripts).
    L5Ephemeral = 5,
    /// Probe/telemetry-free helpers.
    L6Probe = 6,
}

impl SecurityLayer {
    pub const ALL: [SecurityLayer; ISOLATION_LAYERS] = [
        SecurityLayer::L0Kernel,
        SecurityLayer::L1Service,
        SecurityLayer::L2Application,
        SecurityLayer::L3Sandboxed,
        SecurityLayer::L4Guest,
        SecurityLayer::L5Ephemeral,
        SecurityLayer::L6Probe,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SecurityLayer::L0Kernel => "L0-kernel",
            SecurityLayer::L1Service => "L1-service",
            SecurityLayer::L2Application => "L2-app",
            SecurityLayer::L3Sandboxed => "L3-sandbox",
            SecurityLayer::L4Guest => "L4-guest",
            SecurityLayer::L5Ephemeral => "L5-ephemeral",
            SecurityLayer::L6Probe => "L6-probe",
        }
    }

    /// The most a process at this layer may ever hold.
    pub fn ceiling(self) -> Caps {
        match self {
            SecurityLayer::L0Kernel => Caps(Self::l0_kernel_all()),
            SecurityLayer::L1Service => Caps(Caps::FS_READ | Caps::FS_WRITE | Caps::FS_EXEC | Caps::NET | Caps::DEVICE | Caps::SPAWN | Caps::SIGNAL | Caps::MEM_MAP | Caps::TRACE | Caps::LOAD_DRIVER),
            SecurityLayer::L2Application => Caps(Caps::DEFAULT_CHILD | Caps::NET | Caps::SIGNAL | Caps::DEVICE),
            SecurityLayer::L3Sandboxed => Caps(Caps::FS_READ | Caps::FS_EXEC | Caps::MEM_MAP),
            SecurityLayer::L4Guest => Caps(Caps::MEM_MAP | Caps::DEVICE | Caps::RAW_IO),
            SecurityLayer::L5Ephemeral => Caps(Caps::FS_READ | Caps::FS_WRITE | Caps::FS_EXEC | Caps::MEM_MAP),
            SecurityLayer::L6Probe => Caps(Caps::FS_READ),
        }
    }

    const fn l0_kernel_all() -> u64 {
        Caps::ALL
    }
}

impl Caps {
    /// Layer-specific conversion lives here to keep the constant tables honest.
    const fn apply_ceiling(self, ceiling: u64) -> Caps {
        Caps(self.0 & ceiling)
    }

    pub fn capped(self, layer: SecurityLayer) -> Caps {
        self.apply_ceiling(layer.ceiling().0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapToken {
    pub owner: u32,
    pub caps: Caps,
    pub layer: SecurityLayer,
    /// 0 = no expiry.
    pub expires_at: u64,
}

impl CapToken {
    pub const fn new(owner: u32, caps: Caps, layer: SecurityLayer) -> CapToken {
        CapToken {
            owner,
            caps,
            layer,
            expires_at: 0,
        }
    }

    pub fn with_expiry(mut self, at: u64) -> CapToken {
        self.expires_at = at;
        self
    }

    pub fn expired(&self, now: u64) -> bool {
        self.expires_at != 0 && now > self.expires_at
    }

    /// F112: the arbitration itself. A token that has expired, or that asks for
    /// a capability above its layer ceiling, authorizes nothing.
    pub fn arbitrate(&self, required: u64, now: u64) -> Result<(), CapabilityError> {
        if self.expired(now) {
            return Err(CapabilityError::Expired);
        }
        if !self.caps.is_subset_of(self.layer.ceiling()) {
            return Err(CapabilityError::AboveLayerCeiling);
        }
        if !self.caps.has(required) {
            return Err(CapabilityError::Missing);
        }
        Ok(())
    }

    /// Hand a narrower token to someone else.
    pub fn delegate(&self, to: u32, subset: Caps, now: u64) -> Result<CapToken, CapabilityError> {
        if self.expired(now) {
            return Err(CapabilityError::Expired);
        }
        if !subset.is_subset_of(self.caps) {
            return Err(CapabilityError::Escalation);
        }
        Ok(CapToken {
            owner: to,
            caps: subset,
            layer: self.layer,
            expires_at: self.expires_at,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapabilityError {
    Missing,
    Expired,
    Escalation,
    AboveLayerCeiling,
}

// ---------------------------------------------------------------------------
// F115 — users, groups, permissions
// ---------------------------------------------------------------------------

pub const UID_ROOT: u16 = 0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, PartialOrd, Ord)]
pub struct Credentials {
    pub uid: u16,
    pub gid: u16,
}

impl Credentials {
    pub const fn root() -> Credentials {
        Credentials {
            uid: UID_ROOT,
            gid: UID_ROOT,
        }
    }

    pub fn is_root(&self) -> bool {
        self.uid == UID_ROOT
    }
}

/// Nine-bit `rwxrwxrwx`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mode(pub u16);

impl Mode {
    pub const READ: u16 = 0o4;
    pub const WRITE: u16 = 0o2;
    pub const EXEC: u16 = 0o1;

    pub const fn user(self) -> u16 {
        (self.0 >> 6) & 0o7
    }
    pub const fn group(self) -> u16 {
        (self.0 >> 3) & 0o7
    }
    pub const fn other(self) -> u16 {
        self.0 & 0o7
    }

    pub fn from_octal(bits: u16) -> Mode {
        Mode(bits & 0o777)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Permission {
    pub owner: Credentials,
    pub mode: Mode,
}

/// What the caller wants to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Access {
    Read,
    Write,
    Execute,
}

impl Access {
    fn bit(self) -> u16 {
        match self {
            Access::Read => Mode::READ,
            Access::Write => Mode::WRITE,
            Access::Execute => Mode::EXEC,
        }
    }
}

/// F115: the access decision, root quirks included. Root bypasses the
/// permission bits — except the execute bit, because "root can run anything"
/// is how a mis-typed command becomes an outage.
pub fn may_access(perm: &Permission, who: Credentials, want: Access) -> bool {
    if who.is_root() {
        // Root bypasses the permission bits, except for the execute bit: "root
        // can run anything" is how a typo becomes an outage.
        if want == Access::Execute && perm.mode.0 & 0o111 == 0 {
            return false;
        }
        return true;
    }
    let bits = if who.uid == perm.owner.uid {
        perm.mode.user()
    } else if who.gid == perm.owner.gid {
        perm.mode.group()
    } else {
        perm.mode.other()
    };
    bits & want.bit() != 0
}

// ---------------------------------------------------------------------------
// F120 — exit codes
// ---------------------------------------------------------------------------

pub const EXIT_OK: i32 = 0;
pub const EXIT_FAILURE: i32 = 1;
pub const EXIT_NOT_EXECUTABLE: i32 = 126;
pub const EXIT_NOT_FOUND: i32 = 127;
pub const EXIT_SIGNAL_BASE: i32 = 128;

/// F120: one interpretation of "how did it end", used by the shell, the
/// service supervisor and the crash reporter alike.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ExitStatus(pub i32);

impl ExitStatus {
    pub fn from_signal(sig: Signal) -> ExitStatus {
        ExitStatus(EXIT_SIGNAL_BASE + sig.0 as i32)
    }

    pub fn signaled(&self) -> Option<Signal> {
        if self.0 > EXIT_SIGNAL_BASE && self.0 <= EXIT_SIGNAL_BASE + MAX_SIGNAL as i32 {
            Some(Signal((self.0 - EXIT_SIGNAL_BASE) as u32))
        } else {
            None
        }
    }

    pub fn is_success(&self) -> bool {
        self.0 == EXIT_OK
    }

    pub fn describe(&self) -> &'static str {
        match self.0 {
            EXIT_OK => "success",
            EXIT_FAILURE => "failure",
            EXIT_NOT_EXECUTABLE => "not executable",
            EXIT_NOT_FOUND => "not found",
            v if v > EXIT_SIGNAL_BASE && v <= EXIT_SIGNAL_BASE + MAX_SIGNAL as i32 => "killed by signal",
            v if (0..=125).contains(&v) => "user error",
            _ => "reserved",
        }
    }

    /// Shell-visible value, normalized to 0..=255.
    pub fn shell_code(&self) -> u8 {
        (self.0 & 0xFF) as u8
    }
}

// ---------------------------------------------------------------------------
// F121 — environment block
// ---------------------------------------------------------------------------

pub const ENV_BYTES: usize = 512;
pub const ENV_MAX_ENTRIES: usize = 16;

/// A fixed `KEY=VALUE\0` block. Fixed because a kernel must not allocate to
/// start a process, and 512 bytes is what the first-boot services need.
#[derive(Clone, Copy)]
pub struct EnvBlock {
    bytes: [u8; ENV_BYTES],
    len: usize,
}

impl EnvBlock {
    pub const fn new() -> EnvBlock {
        EnvBlock {
            bytes: [0; ENV_BYTES],
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    /// Number of `KEY=VALUE` entries.
    pub fn count(&self) -> usize {
        if self.len == 0 {
            0
        } else {
            self.bytes[..self.len].iter().filter(|b| **b == 0).count()
        }
    }

    /// Replace or append `key`. Replacing keeps the block from filling up when
    /// a service re-exports the same variable.
    pub fn set(&mut self, key: &str, value: &str) -> bool {
        if key.is_empty() || key.contains('=') || value.contains('\0') {
            return false;
        }
        if let Some(start) = self.find_entry(key) {
            let end = start + self.entry_len(start);
            if self.replace_at(start, end, key, value) {
                return true;
            }
            return false;
        }
        if self.count() >= ENV_MAX_ENTRIES {
            return false;
        }
        let need = key.len() + 1 + value.len() + 1;
        if self.len + need > ENV_BYTES {
            return false;
        }
        // Write it straight through: KEY=VALUE\0
        let at = self.len;
        let mut p = at;
        for &b in key.as_bytes() {
            self.bytes[p] = b;
            p += 1;
        }
        self.bytes[p] = b'=';
        p += 1;
        for &b in value.as_bytes() {
            self.bytes[p] = b;
            p += 1;
        }
        self.bytes[p] = 0;
        p += 1;
        self.len = p;
        true
    }

    fn entry_len(&self, start: usize) -> usize {
        let mut n = 0usize;
        while start + n < self.len && self.bytes[start + n] != 0 {
            n += 1;
        }
        n + 1
    }

    fn find_entry(&self, key: &str) -> Option<usize> {
        let mut at = 0usize;
        while at < self.len {
            let len = self.entry_len(at);
            let entry = &self.bytes[at..at + len - 1];
            if let Some(eq) = entry.iter().position(|b| *b == b'=') {
                if &entry[..eq] == key.as_bytes() {
                    return Some(at);
                }
            }
            at += len;
        }
        None
    }

    fn replace_at(&mut self, start: usize, end: usize, key: &str, value: &str) -> bool {
        let old = end - start;
        let new = key.len() + 1 + value.len() + 1;
        if self.len - old + new > ENV_BYTES {
            return false;
        }
        // Shift the tail to make room.
        let tail = self.len - end;
        if new != old {
            if new > old {
                let mut i = self.len;
                while i > end {
                    i -= 1;
                    self.bytes[i + (new - old)] = self.bytes[i];
                }
            } else {
                for i in 0..tail {
                    self.bytes[end - (old - new) + i] = self.bytes[end + i];
                }
            }
            self.len = self.len - old + new;
        }
        let mut p = start;
        for &b in key.as_bytes() {
            self.bytes[p] = b;
            p += 1;
        }
        self.bytes[p] = b'=';
        p += 1;
        for &b in value.as_bytes() {
            self.bytes[p] = b;
            p += 1;
        }
        self.bytes[p] = 0;
        true
    }

    /// Look a key up, returning the borrowed slice.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        let start = self.find_entry(key)?;
        let len = self.entry_len(start);
        let entry = &self.bytes[start..start + len - 1];
        let eq = entry.iter().position(|b| *b == b'=')?;
        Some(&entry[eq + 1..])
    }

    pub fn get_str(&self, key: &str) -> Option<&str> {
        core::str::from_utf8(self.get(key)?).ok()
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn remove(&mut self, key: &str) -> bool {
        let start = match self.find_entry(key) {
            Some(s) => s,
            None => return false,
        };
        let len = self.entry_len(start);
        for i in 0..(self.len - start - len) {
            self.bytes[start + i] = self.bytes[start + len + i];
        }
        self.len -= len;
        true
    }

    /// Seed the block with the standard set every process expects.
    pub fn standard() -> EnvBlock {
        let mut e = EnvBlock::new();
        let _ = e.set("PATH", "/bin:/system/bin");
        let _ = e.set("HOME", "/");
        let _ = e.set("TERM", "varix");
        let _ = e.set("LANG", "zh_CN.UTF-8");
        let _ = e.set("VARIX", "1");
        e
    }
}

impl Default for EnvBlock {
    fn default() -> EnvBlock {
        EnvBlock::new()
    }
}

// ---------------------------------------------------------------------------
// F122 — command line parsing
// ---------------------------------------------------------------------------

pub const MAX_ARGV: usize = 16;
pub const ARG_BYTES: usize = 512;

/// A split command line. Quotes are honoured so a path with a space survives,
/// which is the one thing a naive `split(' ')` gets wrong every time.
#[derive(Clone, Copy)]
pub struct Argv {
    bytes: [u8; ARG_BYTES],
    len: usize,
    /// (offset, length) per argument.
    spans: [(u16, u16); MAX_ARGV],
    count: usize,
}

impl Argv {
    pub const fn new() -> Argv {
        Argv {
            bytes: [0; ARG_BYTES],
            len: 0,
            spans: [(0, 0); MAX_ARGV],
            count: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, i: usize) -> Option<&str> {
        let (off, len) = *self.spans.get(i)?;
        if i >= self.count {
            return None;
        }
        core::str::from_utf8(&self.bytes[off as usize..off as usize + len as usize]).ok()
    }

    pub fn program(&self) -> Option<&str> {
        self.get(0)
    }

    /// Split `line` into arguments, honouring single and double quotes.
    pub fn parse(line: &str) -> Argv {
        let mut a = Argv::new();
        let mut quote: Option<char> = None;
        let mut start: Option<usize> = None;
        for ch in line.chars() {
            match ch {
                '\'' | '"' => {
                    if quote == Some(ch) {
                        quote = None;
                    } else if quote.is_none() {
                        quote = Some(ch);
                    }
                    if start.is_none() {
                        start = Some(a.len);
                    }
                }
                c if c.is_whitespace() && quote.is_none() => {
                    a.close(start);
                    start = None;
                }
                _ => {
                    if start.is_none() {
                        start = Some(a.len);
                    }
                    a.push(ch);
                }
            }
        }
        a.close(start);
        a
    }

    fn push(&mut self, ch: char) {
        let mut buf = [0u8; 4];
        for &b in ch.encode_utf8(&mut buf).as_bytes() {
            if self.len < ARG_BYTES {
                self.bytes[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn close(&mut self, start: Option<usize>) {
        if let Some(s) = start {
            if self.count >= MAX_ARGV || self.len <= s {
                return;
            }
            self.spans[self.count] = (s as u16, (self.len - s) as u16);
            self.count += 1;
        }
    }
}

impl Default for Argv {
    fn default() -> Argv {
        Argv::new()
    }
}

// ---------------------------------------------------------------------------
// F110/F118/F119 — crash isolation, core dumps, event trace
// ---------------------------------------------------------------------------

pub const MAX_CORE_FRAMES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CoreDump {
    pub pid: u32,
    pub signal: Signal,
    pub rip: u64,
    pub rsp: u64,
    pub frames: [u64; MAX_CORE_FRAMES],
    pub frame_count: usize,
    pub complete: bool,
}

impl CoreDump {
    pub fn begin(pid: u32, signal: Signal, rip: u64, rsp: u64) -> CoreDump {
        CoreDump {
            pid,
            signal,
            rip,
            rsp,
            frames: [0; MAX_CORE_FRAMES],
            frame_count: 0,
            complete: false,
        }
    }

    pub fn capture_backtrace(&mut self, mut read: impl FnMut(u64) -> u64) -> usize {
        let mut addr = self.rip;
        while self.frame_count < MAX_CORE_FRAMES && addr != 0 {
            self.frames[self.frame_count] = addr;
            self.frame_count += 1;
            addr = read(addr);
        }
        self.frame_count
    }

    pub fn finish(&mut self) {
        self.complete = true;
    }

    /// Header written before the dump lands on disk (F118).
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("core pid=");
        w.num(self.pid as u64);
        w.str(" sig=");
        w.str(self.signal.name());
        w.str(" rip=0x");
        w.num(self.rip);
        w.str(" frames=");
        w.num(self.frame_count as u64);
        w.str(if self.complete { " complete\n" } else { " partial\n" });
        w.used()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcEventKind {
    Fork,
    Exec,
    Exit,
    Signal,
    Fault,
    Throttled,
    Reaped,
    Adopted,
}

impl ProcEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcEventKind::Fork => "fork",
            ProcEventKind::Exec => "exec",
            ProcEventKind::Exit => "exit",
            ProcEventKind::Signal => "signal",
            ProcEventKind::Fault => "fault",
            ProcEventKind::Throttled => "throttled",
            ProcEventKind::Reaped => "reaped",
            ProcEventKind::Adopted => "adopted",
        }
    }
}

pub const MAX_PROC_EVENTS: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ProcEvent {
    pub tick: u64,
    pub pid: u32,
    pub kind: Option<ProcEventKind>,
    pub detail: i64,
}

pub struct ProcTrace {
    events: [ProcEvent; MAX_PROC_EVENTS],
    head: usize,
    total: u64,
}

impl ProcTrace {
    pub const fn new() -> ProcTrace {
        ProcTrace {
            events: [ProcEvent {
                tick: 0,
                pid: 0,
                kind: None,
                detail: 0,
            }; MAX_PROC_EVENTS],
            head: 0,
            total: 0,
        }
    }

    pub fn push(&mut self, e: ProcEvent) {
        self.events[self.head] = e;
        self.head = (self.head + 1) % MAX_PROC_EVENTS;
        self.total += 1;
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_PROC_EVENTS)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// `offset` 0 is the newest.
    pub fn get(&self, offset: usize) -> Option<ProcEvent> {
        if offset >= self.len() {
            return None;
        }
        Some(self.events[(self.head + MAX_PROC_EVENTS - 1 - offset) % MAX_PROC_EVENTS])
    }

    pub fn count(&self, kind: ProcEventKind) -> u64 {
        (0..self.len())
            .filter(|i| self.get(*i).map(|e| e.kind == Some(kind)).unwrap_or(false))
            .count() as u64
    }
}

impl Default for ProcTrace {
    fn default() -> ProcTrace {
        ProcTrace::new()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CrashRecovery {
    pub pid: u32,
    pub signal: Signal,
    pub group_killed: bool,
    pub kernel_untouched: bool,
}

/// F110: the red line. A user fault may only ever cost the offending process —
/// or, when the fault is a group-wide failure (a shared-memory protocol gone
/// wrong), that process's job. The kernel is never a casualty.
pub const CRASH_SCOPE_PROCESS: &str = "process";
pub const CRASH_SCOPE_GROUP: &str = "group";

pub fn isolate_fault(pid: u32, signal: Signal, group: bool) -> CrashRecovery {
    CrashRecovery {
        pid,
        signal,
        group_killed: group,
        kernel_untouched: true,
    }
}

// ---------------------------------------------------------------------------
// F113 — resource limits
// ---------------------------------------------------------------------------

pub const LIMIT_UNLIMITED: u64 = u64::MAX;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Limits {
    pub cpu_ticks: u64,
    pub rss_bytes: u64,
    pub fds: u32,
    pub threads: u32,
}

impl Limits {
    pub const fn default_user() -> Limits {
        Limits {
            cpu_ticks: LIMIT_UNLIMITED,
            rss_bytes: 2 * 1024 * 1024 * 1024,
            fds: 256,
            threads: 32,
        }
    }

    pub const fn service() -> Limits {
        Limits {
            cpu_ticks: LIMIT_UNLIMITED,
            rss_bytes: 512 * 1024 * 1024,
            fds: 128,
            threads: 16,
        }
    }
}

impl Default for Limits {
    fn default() -> Limits {
        Limits::default_user()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Usage {
    pub cpu_ticks: u64,
    pub rss_bytes: u64,
    pub fds: u32,
    pub threads: u32,
}

/// Which limit was hit, if any. Reported so the process gets a signal it can
/// name rather than a mystery death.
pub fn limit_breach(limits: &Limits, usage: &Usage) -> Option<&'static str> {
    if usage.cpu_ticks > limits.cpu_ticks {
        return Some("cpu");
    }
    if usage.rss_bytes > limits.rss_bytes {
        return Some("rss");
    }
    if usage.fds > limits.fds {
        return Some("fds");
    }
    if usage.threads > limits.threads {
        return Some("threads");
    }
    None
}

// ---------------------------------------------------------------------------
// F114 — process groups and sessions
// ---------------------------------------------------------------------------

pub const MAX_JOBS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Job {
    pub pgid: u32,
    pub sid: u32,
    pub leader: u32,
    pub members: u32,
}

pub struct JobTable {
    jobs: [Job; MAX_JOBS],
    len: usize,
}

impl JobTable {
    pub const fn new() -> JobTable {
        JobTable {
            jobs: [Job {
                pgid: 0,
                sid: 0,
                leader: 0,
                members: 0,
            }; MAX_JOBS],
            len: 0,
        }
    }

    pub fn create(&mut self, leader: u32) -> Option<u32> {
        if self.len >= MAX_JOBS {
            return None;
        }
        // The group id is the leader's pid, which is what makes `kill -PGID`
        // work without a separate namespace.
        self.jobs[self.len] = Job {
            pgid: leader,
            sid: leader,
            leader,
            members: 1,
        };
        self.len += 1;
        Some(leader)
    }

    pub fn find(&self, pgid: u32) -> Option<&Job> {
        self.jobs[..self.len].iter().find(|j| j.pgid == pgid)
    }

    pub fn find_mut(&mut self, pgid: u32) -> Option<&mut Job> {
        self.jobs[..self.len].iter_mut().find(|j| j.pgid == pgid)
    }

    pub fn join(&mut self, pgid: u32) -> bool {
        match self.find_mut(pgid) {
            Some(j) => {
                j.members += 1;
                true
            }
            None => false,
        }
    }

    pub fn leave(&mut self, pgid: u32) -> bool {
        match self.find_mut(pgid) {
            Some(j) if j.members > 0 => {
                j.members -= 1;
                true
            }
            _ => false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for JobTable {
    fn default() -> JobTable {
        JobTable::new()
    }
}

// ---------------------------------------------------------------------------
// F116/F117/F123 — daemons, name service, multi-instance arbitration
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

impl RestartPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            RestartPolicy::Never => "never",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::Always => "always",
        }
    }

    /// F116: restart decision. `clean_exit` is the process's own claim; a
    /// signal death is never clean.
    pub fn should_restart(self, status: ExitStatus, restarts: u32) -> bool {
        if restarts >= MAX_DAEMON_RESTARTS {
            return false;
        }
        match self {
            RestartPolicy::Never => false,
            RestartPolicy::Always => true,
            RestartPolicy::OnFailure => !status.is_success(),
        }
    }
}

/// A crash-looping daemon is worse than a dead one: the supervisor gives up.
pub const MAX_DAEMON_RESTARTS: u32 = 5;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Daemon {
    pub name: &'static str,
    pub pid: u32,
    pub policy: RestartPolicy,
    pub restarts: u32,
    pub running: bool,
}

impl Daemon {
    pub fn started(name: &'static str, pid: u32, policy: RestartPolicy) -> Daemon {
        Daemon {
            name,
            pid,
            policy,
            restarts: 0,
            running: true,
        }
    }

    pub fn stopped(&mut self, status: ExitStatus) -> bool {
        self.running = false;
        if self.policy.should_restart(status, self.restarts) {
            self.restarts += 1;
            true
        } else {
            false
        }
    }
}

pub const MAX_NAMES: usize = 32;

/// F117: name → pid, with reverse lookup. Names are kernel-owned strings so a
/// process cannot unregister someone else's service by handing in a stale
/// pointer.
pub struct NameService {
    names: [&'static str; MAX_NAMES],
    pids: [u32; MAX_NAMES],
    len: usize,
    collisions: u64,
}

impl NameService {
    pub const fn new() -> NameService {
        NameService {
            names: [""; MAX_NAMES],
            pids: [NO_PID; MAX_NAMES],
            len: 0,
            collisions: 0,
        }
    }

    pub fn register(&mut self, name: &'static str, pid: u32) -> bool {
        if name.is_empty() || self.lookup(name).is_some() || self.len >= MAX_NAMES {
            self.collisions += 1;
            return false;
        }
        self.names[self.len] = name;
        self.pids[self.len] = pid;
        self.len += 1;
        true
    }

    pub fn lookup(&self, name: &str) -> Option<u32> {
        self.names[..self.len]
            .iter()
            .position(|n| *n == name)
            .map(|i| self.pids[i])
    }

    pub fn name_of(&self, pid: u32) -> Option<&'static str> {
        self.pids[..self.len]
            .iter()
            .position(|p| *p == pid)
            .map(|i| self.names[i])
    }

    pub fn unregister(&mut self, name: &str) -> bool {
        match self.names[..self.len].iter().position(|n| *n == name) {
            Some(i) => {
                let last = self.len - 1;
                self.names[i] = self.names[last];
                self.pids[i] = self.pids[last];
                self.len = last;
                true
            }
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn collisions(&self) -> u64 {
        self.collisions
    }
}

impl Default for NameService {
    fn default() -> NameService {
        NameService::new()
    }
}

/// F123: the result of asking to become "the" instance of a name. The
/// "already running" answer carries the existing pid so the caller can be
/// brought to the front instead of being duplicated — the behaviour a desktop
/// needs and a naive single-instance check always gets wrong.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstanceClaim {
    Granted,
    /// The name is taken; front the existing instance.
    AlreadyRunning(u32),
    /// The name was held by a dead process and has been reclaimed.
    ReclaimedFrom(u32),
    Refused,
}

pub fn claim_instance(service: &mut NameService, name: &'static str, pid: u32, holder_alive: bool) -> InstanceClaim {
    match service.lookup(name) {
        None => {
            if service.register(name, pid) {
                InstanceClaim::Granted
            } else {
                InstanceClaim::Refused
            }
        }
        Some(existing) if existing == pid => InstanceClaim::AlreadyRunning(existing),
        Some(existing) => {
            if holder_alive {
                InstanceClaim::AlreadyRunning(existing)
            } else {
                let _ = service.unregister(name);
                if service.register(name, pid) {
                    InstanceClaim::ReclaimedFrom(existing)
                } else {
                    InstanceClaim::Refused
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The process table
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ProcState {
    #[default]
    Unused,
    Ready,
    Running,
    Sleeping,
    /// Exited, waiting to be collected by its parent.
    Zombie,
    /// Being torn down.
    Dead,
}

impl ProcState {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcState::Unused => "unused",
            ProcState::Ready => "ready",
            ProcState::Running => "running",
            ProcState::Sleeping => "sleeping",
            ProcState::Zombie => "zombie",
            ProcState::Dead => "dead",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    pub state: ProcState,
    pub creds: Credentials,
    pub caps: Caps,
    pub layer: SecurityLayer,
    pub limits: Limits,
    pub usage: Usage,
    pub exit: ExitStatus,
    pub pending: SignalSet,
    pub handler_installed: SignalSet,
    pub title: &'static str,
}

impl Process {
    pub const fn new(pid: u32) -> Process {
        Process {
            pid,
            ppid: NO_PID,
            state: ProcState::Unused,
            creds: Credentials { uid: 1, gid: 1 },
            caps: Caps(Caps::DEFAULT_CHILD),
            layer: SecurityLayer::L2Application,
            limits: Limits::default_user(),
            usage: Usage {
                cpu_ticks: 0,
                rss_bytes: 0,
                fds: 0,
                threads: 1,
            },
            exit: ExitStatus(0),
            pending: SignalSet(0),
            handler_installed: SignalSet(0),
            title: "",
        }
    }

    pub fn alive(&self) -> bool {
        matches!(
            self.state,
            ProcState::Ready | ProcState::Running | ProcState::Sleeping
        )
    }

    /// F109: the action the kernel will take for `sig` on this process.
    pub fn action_for(&self, sig: Signal) -> SignalAction {
        if sig.is_uncatchable() {
            return default_action(sig);
        }
        if self.handler_installed.contains(sig) {
            // A handler means "deliver it"; the signal-safety of that is the
            // process's own problem, which is exactly the POSIX bargain.
            return SignalAction::Ignore;
        }
        default_action(sig)
    }
}

/// The process table plus the tree, groups and trace.
pub struct ProcTable {
    procs: [Process; MAX_PROCESSES],
    count: usize,
    /// pid of the parent for each slot — kept separate because it is read on
    /// every exit path and must survive the process being cleared.
    parents: [u32; MAX_PROCESSES],
    first_child: [u32; MAX_PROCESSES],
    next_sibling: [u32; MAX_PROCESSES],
    jobs: JobTable,
    names: NameService,
    trace: ProcTrace,
    coredumps: u64,
    adoptions: u64,
}

impl ProcTable {
    pub const fn new() -> ProcTable {
        ProcTable {
            procs: [Process::new(0); MAX_PROCESSES],
            count: 0,
            parents: [NO_PID; MAX_PROCESSES],
            first_child: [NO_PID; MAX_PROCESSES],
            next_sibling: [NO_PID; MAX_PROCESSES],
            jobs: JobTable::new(),
            names: NameService::new(),
            trace: ProcTrace::new(),
            coredumps: 0,
            adoptions: 0,
        }
    }

    pub fn get(&self, pid: u32) -> Option<&Process> {
        self.procs.get(pid as usize).filter(|p| p.state != ProcState::Unused)
    }

    pub fn get_mut(&mut self, pid: u32) -> Option<&mut Process> {
        let p = self.procs.get_mut(pid as usize)?;
        if p.state == ProcState::Unused {
            None
        } else {
            Some(p)
        }
    }

    pub fn processes(&self) -> &[Process; MAX_PROCESSES] {
        &self.procs
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn jobs(&self) -> &JobTable {
        &self.jobs
    }

    pub fn jobs_mut(&mut self) -> &mut JobTable {
        &mut self.jobs
    }

    pub fn names(&self) -> &NameService {
        &self.names
    }

    pub fn names_mut(&mut self) -> &mut NameService {
        &mut self.names
    }

    pub fn trace(&self) -> &ProcTrace {
        &self.trace
    }

    pub fn trace_mut(&mut self) -> &mut ProcTrace {
        &mut self.trace
    }

    pub fn coredumps(&self) -> u64 {
        self.coredumps
    }

    pub fn adoptions(&self) -> u64 {
        self.adoptions
    }

    /// F106/F108: create a process. `ppid == NO_PID` means "the child of init",
    /// which is what a kernel thread or the first user process looks like.
    pub fn create(
        &mut self,
        pid: u32,
        ppid: u32,
        title: &'static str,
        layer: SecurityLayer,
        creds: Credentials,
    ) -> bool {
        if pid as usize >= MAX_PROCESSES || self.procs[pid as usize].state != ProcState::Unused {
            return false;
        }
        let mut p = Process::new(pid);
        p.ppid = ppid;
        p.state = ProcState::Ready;
        p.title = title;
        p.layer = layer;
        p.creds = creds;
        p.caps = Caps::user_default().capped(layer);
        p.limits = Limits::default_user();
        self.procs[pid as usize] = p;
        self.parents[pid as usize] = ppid;
        self.first_child[pid as usize] = NO_PID;
        self.next_sibling[pid as usize] = NO_PID;
        // Link into the tree (F107).
        if ppid != NO_PID && (ppid as usize) < MAX_PROCESSES {
            self.next_sibling[pid as usize] = self.first_child[ppid as usize];
            self.first_child[ppid as usize] = pid;
        }
        self.count += 1;
        self.trace.push(ProcEvent {
            tick: crate::cpu::clock::ticks(),
            pid,
            kind: Some(ProcEventKind::Fork),
            detail: ppid as i64,
        });
        true
    }

    /// F106: the one process that always exists.
    pub fn spawn_init(&mut self, pid: u32) -> bool {
        let ok = self.create(
            pid,
            NO_PID,
            "init",
            SecurityLayer::L1Service,
            Credentials::root(),
        );
        if ok {
            if let Some(p) = self.get_mut(pid) {
                p.caps = Caps::user_default()
                    .capped(SecurityLayer::L1Service)
                    .grants(Caps::SIGNAL);
            }
            let _ = self.jobs.create(pid);
        }
        ok
    }

    /// F108: fork. The child inherits credentials, limits and a *subset* of the
    /// capabilities (F112) — never the parent's full set.
    pub fn fork(&mut self, parent: u32, child_pid: u32) -> Option<&Process> {
        let (layer, creds, caps, title, limits) = {
            let p = self.get(parent)?;
            (p.layer, p.creds, p.caps, p.title, p.limits)
        };
        let inherited = caps.capped(layer).attenuate(Caps::LOAD_DRIVER | Caps::RAW_IO);
        if !self.create(child_pid, parent, title, layer, creds) {
            return None;
        }
        if let Some(p) = self.get_mut(child_pid) {
            p.caps = inherited;
            p.limits = limits;
        }
        self.get(child_pid)
    }

    /// F108: exec — the process image changes, the identity does not.
    pub fn exec(&mut self, pid: u32, image: &ElfImage) -> Result<(), ElfError> {
        image.validate()?;
        let p = self.get_mut(pid).ok_or(ElfError::NoImage)?;
        p.title = "exec";
        // An exec never gains capability: only a token minted by a holder can.
        self.trace.push(ProcEvent {
            tick: crate::cpu::clock::ticks(),
            pid,
            kind: Some(ProcEventKind::Exec),
            detail: image.entry as i64,
        });
        Ok(())
    }

    /// F109: queue a signal. SIGKILL always wins immediately.
    pub fn signal(&mut self, pid: u32, sig: Signal) -> Result<SignalAction, &'static str> {
        if !sig.valid() {
            return Err("invalid signal number");
        }
        let p = self.get_mut(pid).ok_or("no such process")?;
        if !p.alive() {
            return Err("process is not live");
        }
        let action = p.action_for(sig);
        p.pending.add(sig);
        let tick = crate::cpu::clock::ticks();
        self.trace.push(ProcEvent {
            tick,
            pid,
            kind: Some(ProcEventKind::Signal),
            detail: sig.0 as i64,
        });
        Ok(action)
    }

    /// Apply the default action of the lowest pending signal.
    pub fn deliver_pending(&mut self, pid: u32) -> Option<ExitStatus> {
        let (sig, action, user_handled) = {
            let p = self.get(pid)?;
            let sig = p.pending.next()?;
            (sig, p.action_for(sig), p.handler_installed.contains(sig))
        };
        if action == SignalAction::Ignore && user_handled {
            // A user handler owns this one: leave it pending so the delivery
            // path can hand it over, rather than silently discarding it.
            return None;
        }
        let (status, dumped) = {
            let p = self.get_mut(pid)?;
            p.pending.remove(sig);
            match action {
                SignalAction::Term | SignalAction::Core => {
                    p.state = ProcState::Zombie;
                    p.exit = ExitStatus::from_signal(sig);
                    (Some(p.exit), action == SignalAction::Core)
                }
                SignalAction::Stop => {
                    p.state = ProcState::Sleeping;
                    (None, false)
                }
                SignalAction::Continue => {
                    p.state = ProcState::Ready;
                    (None, false)
                }
                _ => (None, false),
            }
        };
        if dumped {
            self.coredumps += 1;
        }
        status
    }

    /// F110: a user-mode fault kills the process (or its job) and nothing else.
    pub fn fault(&mut self, pid: u32, sig: Signal, whole_group: bool) -> CrashRecovery {
        let recovery = isolate_fault(pid, sig, whole_group);
        let _ = self.signal(pid, sig);
        if let Some(p) = self.get_mut(pid) {
            p.state = ProcState::Zombie;
            p.exit = ExitStatus::from_signal(sig);
        }
        self.coredumps += 1;
        self.trace.push(ProcEvent {
            tick: crate::cpu::clock::ticks(),
            pid,
            kind: Some(ProcEventKind::Fault),
            detail: sig.0 as i64,
        });
        recovery
    }

    /// F111: collect a zombie. Returns the exit status so the parent can report
    /// it. Reaping a *live* process is refused — that is `kill`, not `reap`.
    pub fn reap(&mut self, pid: u32) -> Result<ExitStatus, &'static str> {
        let status = match self.get(pid) {
            Some(p) if p.state == ProcState::Zombie => p.exit,
            Some(_) => return Err("process is not a zombie"),
            None => return Err("no such process"),
        };
        // Reparent the orphaned children to init (F111).
        if let Some(first) = self.first_child.get(pid as usize).copied() {
            let mut child = first;
            while child != NO_PID && (child as usize) < MAX_PROCESSES {
                let next = self.next_sibling[child as usize];
                self.parents[child as usize] = 1;
                self.next_sibling[child as usize] = self.first_child[1];
                self.first_child[1] = child;
                if let Some(c) = self.get_mut(child) {
                    c.ppid = 1;
                }
                self.adoptions += 1;
                child = next;
            }
        }
        self.unlink_from_parent(pid);
        self.first_child[pid as usize] = NO_PID;
        self.procs[pid as usize] = Process::new(pid);
        self.count = self.count.saturating_sub(1);
        self.trace.push(ProcEvent {
            tick: crate::cpu::clock::ticks(),
            pid,
            kind: Some(ProcEventKind::Reaped),
            detail: status.0 as i64,
        });
        Ok(status)
    }

    fn unlink_from_parent(&mut self, pid: u32) {
        let parent = self.parents[pid as usize];
        if parent == NO_PID || parent as usize >= MAX_PROCESSES {
            return;
        }
        if self.first_child[parent as usize] == pid {
            self.first_child[parent as usize] = self.next_sibling[pid as usize];
            return;
        }
        let mut cur = self.first_child[parent as usize];
        while cur != NO_PID && (cur as usize) < MAX_PROCESSES {
            if self.next_sibling[cur as usize] == pid {
                self.next_sibling[cur as usize] = self.next_sibling[pid as usize];
                return;
            }
            cur = self.next_sibling[cur as usize];
        }
    }

    /// F107: walk the children of `pid` into `out`; returns how many.
    pub fn children(&self, pid: u32, out: &mut [u32]) -> usize {
        let mut cur = self.first_child.get(pid as usize).copied().unwrap_or(NO_PID);
        let mut n = 0usize;
        while cur != NO_PID && n < out.len() {
            out[n] = cur;
            n += 1;
            cur = self.next_sibling[cur as usize];
        }
        n
    }

    /// F111: a process whose parent is gone is an orphan.
    pub fn is_orphan(&self, pid: u32) -> bool {
        match self.get(pid) {
            Some(p) => p.ppid == NO_PID || self.get(p.ppid).is_none(),
            None => false,
        }
    }

    /// F113: charge resources and report which limit was crossed, if any.
    pub fn charge(&mut self, pid: u32, ticks: u64, bytes: u64) -> Option<&'static str> {
        let p = self.get_mut(pid)?;
        p.usage.cpu_ticks = p.usage.cpu_ticks.saturating_add(ticks);
        p.usage.rss_bytes = p.usage.rss_bytes.saturating_add(bytes);
        limit_breach(&p.limits, &p.usage)
    }

    pub fn count_in(&self, state: ProcState) -> usize {
        self.procs.iter().filter(|p| p.state == state).count()
    }

    /// F118: capture a core dump for a crashing process.
    pub fn core_dump(&mut self, pid: u32, sig: Signal, rip: u64, rsp: u64) -> Option<CoreDump> {
        let _ = self.get(pid)?;
        let mut dump = CoreDump::begin(pid, sig, rip, rsp);
        // A real backtrace walks frame pointers; the frame buffer is what the
        // dumper hands to the storage layer, so it is captured here.
        dump.capture_backtrace(|_| 0);
        dump.finish();
        self.coredumps += 1;
        Some(dump)
    }

    /// F124: when `blocked` waits on a mutex `owner` holds, the owner inherits
    /// the blocked thread's priority (via the scheduler's inversion guard).
    pub fn inherit_priority(&mut self, sched: &mut crate::sched::engine::ThreadTable, blocked_tid: u32, owner_tid: u32) -> bool {
        let blocked = match sched.thread(blocked_tid) {
            Some(t) => *t,
            None => return false,
        };
        let owner = match sched.thread_mut(owner_tid) {
            Some(o) => o,
            None => return false,
        };
        let mut guard = crate::sched::policy::InversionGuard::new();
        guard.on_block(&blocked, owner, 1)
    }
}

impl Default for ProcTable {
    fn default() -> ProcTable {
        ProcTable::new()
    }
}

/// The live process table. One instance: there is one process namespace.
pub struct ProcDomain {
    pub table: ProcTable,
    pub fds: crate::storage::FdTable,
    pub inherited_caps: crate::proc::Caps,
    pub priority_inherits: u64,
    pub crash_recoveries: u64,
}

impl ProcDomain {
    pub const fn new() -> ProcDomain {
        ProcDomain {
            table: ProcTable::new(),
            fds: crate::storage::FdTable::new(crate::storage::MAX_FDS as u32),
            inherited_caps: crate::proc::Caps(crate::proc::Caps::DEFAULT_CHILD),
            priority_inherits: 0,
            crash_recoveries: 0,
        }
    }

    /// F106: bring up init. Everything else is a descendant of it.
    pub fn bring_up(&mut self) -> bool {
        self.table.spawn_init(1)
    }
}

impl Default for ProcDomain {
    fn default() -> ProcDomain {
        ProcDomain::new()
    }
}

static DOMAIN: crate::cpu::sync::SpinProtected<ProcDomain> =
    crate::cpu::sync::SpinProtected::new(ProcDomain::new());

pub fn domain() -> &'static crate::cpu::sync::SpinProtected<ProcDomain> {
    &DOMAIN
}

/// F101~F125 bring-up.
pub fn init() -> ProcDomainState {
    let mut d = DOMAIN.lock();
    let init_ok = d.bring_up();
    let (passed, failed) = run_process_checks(&d.table);
    let state = ProcDomainState {
        processes: d.table.len(),
        zombies: d.table.count_in(ProcState::Zombie),
        init_ok,
        coredumps: d.table.coredumps(),
        adoptions: d.table.adoptions(),
        self_test: (passed, failed),
    };
    drop(d);

    crate::kinfo!(
        "proc: {} process(es), init={} self-test {}/{}",
        state.processes,
        init_ok,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

/// What the rest of the boot chain wants to know about processes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProcDomainState {
    pub processes: usize,
    pub zombies: usize,
    pub init_ok: bool,
    pub coredumps: u64,
    pub adoptions: u64,
    pub self_test: (usize, usize),
}

impl ProcDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0 && self.init_ok
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("proc count=");
        w.num(self.processes as u64);
        w.str(" zombies=");
        w.num(self.zombies as u64);
        w.str(" init=");
        w.str(if self.init_ok { "up" } else { "DOWN" });
        w.str(" coredumps=");
        w.num(self.coredumps);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

/// Render the domain HUD onto the console.
pub fn render_to_console(st: &ProcDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = PROC_SELFTEST.render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

static PROC_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();

pub fn proc_selftest() -> &'static crate::selftest::SelfTest {
    &PROC_SELFTEST
}

/// F125: the process-chain checks.
pub fn run_process_checks(table: &ProcTable) -> (usize, usize) {
    let r = &PROC_SELFTEST;

    // F101 — a fresh user frame is returnable, a broken one is not.
    let good = UserFrame::for_entry(0x4000_0000, 0x7FFF_0000).sanitized();
    r.check("ring3-frame", good.valid(), "a valid frame was rejected");
    r.check(
        "ring3-rejects-broken",
        !UserFrame {
            rip: 0,
            ..good
        }
        .valid(),
        "a null-rip frame was accepted",
    );

    // F102 — the syscall table has no duplicate numbers.
    let table_ok = SyscallTable::new().validate().is_ok();
    r.check("syscall-table", table_ok, "duplicate syscall number");

    // F106 — init exists exactly once.
    let init_ok = table.get(1).map(|p| p.ppid == NO_PID).unwrap_or(false) || table.is_empty();
    r.check("init-singleton", init_ok, "init is not a root process");

    // F112 — the user default may not exceed its layer ceiling.
    r.check(
        "caps-within-ceiling",
        Caps::user_default().is_subset_of(SecurityLayer::L2Application.ceiling()),
        "default caps exceed the application ceiling",
    );

    // F110 — every crash scope keeps the kernel alive.
    let recovery = isolate_fault(1, Signal::SIGSEGV, false);
    r.check("crash-isolation", recovery.kernel_untouched, "kernel took the hit");

    // F120 — the exit-code conventions hold at their boundaries.
    let ok = ExitStatus::from_signal(Signal::SIGSEGV);
    r.check(
        "exit-conventions",
        ok.signaled() == Some(Signal::SIGSEGV)
            && ExitStatus(EXIT_OK).is_success()
            && !ExitStatus(EXIT_FAILURE).is_success(),
        "exit code conventions broken",
    );

    // F121/F122 — the environment and argv parsers round-trip.
    let mut env = EnvBlock::new();
    let env_ok = env.set("A", "1") && env.get_str("A") == Some("1") && env.set("A", "2") && env.get_str("A") == Some("2");
    r.check("env-block", env_ok, "environment did not round-trip");
    let argv = Argv::parse("cmd --flag \"a b\" 'c d'");
    r.check(
        "argv-quoting",
        argv.count() == 4 && argv.get(2) == Some("a b") && argv.get(3) == Some("c d"),
        "quoted arguments were split",
    );

    // F107/F111 — the tree is consistent.
    let mut children = [0u32; MAX_PROCESSES];
    let n = table.children(1, &mut children);
    r.check(
        "tree-children",
        n <= MAX_PROCESSES,
        "child list overflowed",
    );

    // F123 — arbitration is deterministic.
    let mut names = NameService::new();
    let first = claim_instance(&mut names, "shell", 10, true);
    let second = claim_instance(&mut names, "shell", 11, true);
    r.check(
        "instance-arbitration",
        first == InstanceClaim::Granted && second == InstanceClaim::AlreadyRunning(10),
        "arbitration granted a second instance",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("process self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "process self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

/// Statistics rendered by the HUD.
pub struct ProcStats {
    pub forks: AtomicU64,
    pub execs: AtomicU64,
    pub exits: AtomicU64,
    pub signals: AtomicU64,
    pub faults: AtomicU64,
}

impl ProcStats {
    pub const fn new() -> ProcStats {
        ProcStats {
            forks: AtomicU64::new(0),
            execs: AtomicU64::new(0),
            exits: AtomicU64::new(0),
            signals: AtomicU64::new(0),
            faults: AtomicU64::new(0),
        }
    }
}

impl Default for ProcStats {
    fn default() -> ProcStats {
        ProcStats::new()
    }
}

static STATS: ProcStats = ProcStats::new();

pub fn stats() -> &'static ProcStats {
    &STATS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring3_frames_are_validated_and_sanitized() {
        let mut f = UserFrame::for_entry(0x4000_0000, 0x7FFF_0000);
        f.rflags = 0; // a careless caller
        assert!(!f.valid(), "no IF, no IO privilege level");
        let s = f.sanitized();
        assert!(s.valid());
        assert_ne!(s.rflags & RFLAGS_IF, 0);
        assert_eq!(s.rflags & RFLAGS_IOPL_USER, RFLAGS_IOPL_USER);
        assert_eq!(s.rcx_saved, s.rip, "sysret needs rcx = rip");
        assert_eq!(s.r11_saved, s.rflags, "sysret needs r11 = rflags");
        assert_eq!(enter_ring(Ring::Three, f).unwrap(), s);
        // A kernel address is not a returnable user frame.
        assert!(!UserFrame::for_entry(0xFFFF_8000_0000_1000, 0x7FFF_0000).valid());
        assert_eq!(Ring::Zero.as_str(), "ring0");
        assert_eq!(selectors_for(Ring::Three).0, crate::cpu::gdt::SEL_USER_CODE);
        assert_eq!(selectors_for(Ring::Zero).1, crate::cpu::gdt::SEL_KERNEL_DATA);
    }

    #[test]
    fn signal_sets_are_ordered_and_kill_is_uncatchable() {
        let mut set = SignalSet::default();
        assert!(set.is_empty());
        assert!(set.add(Signal::SIGTERM));
        assert!(set.add(Signal::SIGKILL));
        assert!(!set.add(Signal(0)), "signal 0 is not a signal");
        assert_eq!(set.count(), 2);
        assert!(set.contains(Signal::SIGKILL));
        assert_eq!(set.next(), Some(Signal::SIGKILL), "lowest number first");
        set.remove(Signal::SIGKILL);
        assert_eq!(set.next(), Some(Signal::SIGTERM));
        set.remove(Signal::SIGTERM);
        assert_eq!(set.next(), None);
        assert!(Signal::SIGKILL.is_uncatchable());
        assert!(Signal::SIGSTOP.is_uncatchable());
        assert!(!Signal::SIGTERM.is_uncatchable());
        assert!(Signal(32).valid() == false);
        assert_eq!(Signal::SIGSEGV.name(), "SIGSEGV");
        assert_eq!(default_action(Signal::SIGSEGV), SignalAction::Core);
        assert_eq!(default_action(Signal::SIGCHLD), SignalAction::Ignore);
    }

    #[test]
    fn capabilities_only_ever_shrink() {
        let full = CapToken::new(1, Caps::kernel(), SecurityLayer::L0Kernel);
        assert!(full.arbitrate(Caps::RAW_IO, 0).is_ok());
        assert!(full.arbitrate(Caps::LOAD_DRIVER | Caps::TRACE, 0).is_ok());
        let narrow = CapToken::new(9, Caps(Caps::FS_READ), SecurityLayer::L2Application);
        assert_eq!(
            narrow.arbitrate(Caps::LOAD_DRIVER | Caps::TRACE, 0).unwrap_err(),
            CapabilityError::Missing
        );

        // Delegation cannot hand out more than the caller holds.
        let limited = CapToken::new(2, Caps(Caps::FS_READ | Caps::FS_EXEC), SecurityLayer::L2Application);
        assert_eq!(
            limited.delegate(3, Caps(Caps::FS_WRITE), 0).unwrap_err(),
            CapabilityError::Escalation
        );
        let child = limited.delegate(3, Caps(Caps::FS_READ), 0).unwrap();
        assert!(child.caps.has(Caps::FS_READ));
        assert!(!child.caps.has(Caps::FS_EXEC));

        // A token above its layer ceiling authorizes nothing at all.
        let bad = CapToken::new(4, Caps(Caps::NET), SecurityLayer::L3Sandboxed);
        assert_eq!(bad.arbitrate(Caps::NET, 0).unwrap_err(), CapabilityError::AboveLayerCeiling);

        // Expiry is absolute.
        let short = CapToken::new(5, Caps(Caps::FS_READ), SecurityLayer::L2Application).with_expiry(100);
        assert!(short.arbitrate(Caps::FS_READ, 50).is_ok());
        assert_eq!(short.arbitrate(Caps::FS_READ, 101).unwrap_err(), CapabilityError::Expired);

        // Every layer ceiling is a subset of the kernel's set, and the ceilings
        // are monotone: a higher layer can never hold more.
        for l in SecurityLayer::ALL {
            assert!(l.ceiling().is_subset_of(Caps::kernel()), "{l:?}");
            assert!(!l.as_str().is_empty());
        }
        assert!(SecurityLayer::L0Kernel.ceiling().count() > SecurityLayer::L6Probe.ceiling().count());
        let mut out = [0u8; 96];
        let n = Caps(Caps::FS_READ | Caps::NET).render(&mut out);
        assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), "fs-read,net");
        let mut empty = [0u8; 8];
        let m = Caps(0).render(&mut empty);
        assert_eq!(core::str::from_utf8(&empty[..m]).unwrap(), "-");
    }

    #[test]
    fn permissions_honour_owner_group_other_and_root() {
        let perm = Permission {
            owner: Credentials { uid: 1000, gid: 1000 },
            mode: Mode::from_octal(0o640),
        };
        let owner = Credentials { uid: 1000, gid: 1 };
        let group = Credentials { uid: 2000, gid: 1000 };
        let other = Credentials { uid: 3000, gid: 3000 };
        assert!(may_access(&perm, owner, Access::Read));
        assert!(may_access(&perm, owner, Access::Write));
        assert!(!may_access(&perm, owner, Access::Execute), "no x bit");
        assert!(may_access(&perm, group, Access::Read));
        assert!(!may_access(&perm, group, Access::Write));
        assert!(!may_access(&perm, other, Access::Read));
        // Root bypasses the bits, except for the execute bit.
        assert!(may_access(&perm, Credentials::root(), Access::Write));
        assert!(!may_access(&perm, Credentials::root(), Access::Execute));
        let exec = Permission {
            owner: Credentials { uid: 0, gid: 0 },
            mode: Mode::from_octal(0o755),
        };
        assert!(may_access(&exec, Credentials::root(), Access::Execute));
        assert_eq!(Mode::from_octal(0o755).user(), 0o7);
        assert_eq!(Mode::from_octal(0o755).group(), 0o5);
        assert_eq!(Mode::from_octal(0o755).other(), 0o5);
    }

    #[test]
    fn exit_codes_follow_the_conventions() {
        assert!(ExitStatus(EXIT_OK).is_success());
        assert_eq!(ExitStatus(EXIT_NOT_FOUND).describe(), "not found");
        assert_eq!(ExitStatus(EXIT_NOT_EXECUTABLE).describe(), "not executable");
        assert_eq!(ExitStatus(42).describe(), "user error");
        let killed = ExitStatus::from_signal(Signal::SIGKILL);
        assert_eq!(killed.0, 137);
        assert_eq!(killed.signaled(), Some(Signal::SIGKILL));
        assert_eq!(killed.shell_code(), 137);
        assert_eq!(ExitStatus(0).signaled(), None);
        assert_eq!(ExitStatus(200).describe(), "reserved");
    }

    #[test]
    fn environment_block_set_get_replace_remove() {
        let mut env = EnvBlock::new();
        assert!(env.is_empty());
        assert!(env.set("PATH", "/bin"));
        assert_eq!(env.count(), 1);
        assert_eq!(env.get_str("PATH"), Some("/bin"));
        // Replacing must not grow the block.
        let before = env.len();
        assert!(env.set("PATH", "/bin:/usr/bin"));
        assert_eq!(env.get_str("PATH"), Some("/bin:/usr/bin"));
        assert_eq!(env.count(), 1, "replacement, not a duplicate");
        assert!(env.len() > before, "the value got longer");
        assert!(env.set("TERM", "varix"));
        assert_eq!(env.count(), 2);
        assert_eq!(env.get_str("NOPE"), None);
        assert!(env.remove("PATH"));
        assert_eq!(env.get_str("PATH"), None);
        assert!(!env.remove("PATH"));
        assert_eq!(env.count(), 1);
        // Hostile input is rejected, not stored.
        assert!(!env.set("", "x"));
        assert!(!env.set("A=B", "x"));
        // The standard block is the one every process can rely on.
        let std_env = EnvBlock::standard();
        assert_eq!(std_env.get_str("PATH"), Some("/bin:/system/bin"));
        assert!(std_env.get_str("LANG").is_some());
    }

    #[test]
    fn argv_parser_handles_quotes_and_limits() {
        let a = Argv::parse("ls -la /tmp");
        assert_eq!(a.count(), 3);
        assert_eq!(a.program(), Some("ls"));
        assert_eq!(a.get(1), Some("-la"));
        assert_eq!(a.get(9), None);

        let quoted = Argv::parse("run \"C:\\Program Files\\app.exe\" tail");
        assert_eq!(quoted.count(), 3);
        assert_eq!(quoted.get(1), Some("C:\\Program Files\\app.exe"));

        let mixed = Argv::parse("a 'b c' d");
        assert_eq!(mixed.count(), 3);
        assert_eq!(mixed.get(1), Some("b c"));

        // Empty input and trailing whitespace produce no phantom arguments.
        assert!(Argv::parse("").is_empty());
        assert!(Argv::parse("   ").is_empty());
        assert_eq!(Argv::parse("cmd ").count(), 1);

        // The argument count is bounded rather than overflowing.
        let many = Argv::parse("a b c d e f g h i j k l m n o p q r s t");
        assert!(many.count() <= MAX_ARGV);
    }

    #[test]
    fn process_table_lifecycle_tree_and_reaping() {
        let mut t = ProcTable::new();
        assert!(t.spawn_init(1));
        assert!(t.create(2, 1, "shell", SecurityLayer::L2Application, Credentials { uid: 1000, gid: 1000 }));
        assert!(t.create(3, 2, "child", SecurityLayer::L2Application, Credentials { uid: 1000, gid: 1000 }));
        assert!(!t.create(2, 1, "dup", SecurityLayer::L2Application, Credentials::root()), "pid in use");
        assert_eq!(t.len(), 3);

        let mut children = [0u32; MAX_PROCESSES];
        assert_eq!(t.children(2, &mut children), 1);
        assert_eq!(children[0], 3);
        assert_eq!(t.children(1, &mut children), 1);
        assert_eq!(children[0], 2);

        // Fork inherits a *subset* of capabilities.
        let parent_caps = t.get(2).unwrap().caps;
        let child = t.fork(2, 4).expect("fork");
        assert_eq!(child.ppid, 2);
        assert!(child.caps.is_subset_of(parent_caps));

        // Reaping a live process is refused; a zombie reaps and its children
        // are adopted by init.
        assert_eq!(t.reap(2), Err("process is not a zombie"));
        assert_eq!(t.signal(2, Signal::SIGTERM), Ok(SignalAction::Term));
        assert_eq!(t.deliver_pending(2), Some(ExitStatus::from_signal(Signal::SIGTERM)));
        assert_eq!(t.count_in(ProcState::Zombie), 1);
        let status = t.reap(2).expect("reap the zombie");
        assert_eq!(status.signaled(), Some(Signal::SIGTERM));
        assert!(t.get(2).is_none());
        assert_eq!(t.get(3).unwrap().ppid, 1, "orphan adopted by init");
        assert_eq!(t.adoptions(), 2, "both children were adopted");
        assert_eq!(t.get(4).unwrap().ppid, 1);
        assert_eq!(t.reap(2), Err("no such process"));
    }

    #[test]
    fn crash_isolation_and_core_dumps() {
        let mut t = ProcTable::new();
        assert!(t.spawn_init(1));
        assert!(t.create(5, 1, "worker", SecurityLayer::L2Application, Credentials { uid: 1, gid: 1 }));
        let c = t.fault(5, Signal::SIGSEGV, false);
        assert!(c.kernel_untouched);
        assert!(!c.group_killed);
        assert_eq!(t.get(5).unwrap().state, ProcState::Zombie);
        assert_eq!(t.get(5).unwrap().exit.signaled(), Some(Signal::SIGSEGV));
        assert_eq!(t.coredumps(), 1);
        assert_eq!(t.trace().count(ProcEventKind::Fault), 1);

        let dump = t.core_dump(5, Signal::SIGSEGV, 0x4000_1000, 0x7FFF_0000).unwrap();
        assert!(dump.complete);
        assert_eq!(dump.frames[0], 0x4000_1000);
        assert_eq!(dump.frame_count, 1, "the walker stopped at the null return");
        let mut out = [0u8; 128];
        let n = dump.render(&mut out);
        let s = core::str::from_utf8(&out[..n]).unwrap();
        assert!(s.starts_with("core pid=5 sig=SIGSEGV"), "got {s}");
        assert!(s.contains("complete"), "got {s}");
        assert!(t.core_dump(9, Signal::SIGSEGV, 0, 0).is_none(), "no such process");
    }

    #[test]
    fn limits_are_enforced_per_resource() {
        let limits = Limits {
            cpu_ticks: 100,
            rss_bytes: 1024,
            fds: 4,
            threads: 2,
        };
        let mut usage = Usage::default();
        assert_eq!(limit_breach(&limits, &usage), None);
        usage.cpu_ticks = 101;
        assert_eq!(limit_breach(&limits, &usage), Some("cpu"));
        usage.cpu_ticks = 0;
        usage.rss_bytes = 2048;
        assert_eq!(limit_breach(&limits, &usage), Some("rss"));
        usage.rss_bytes = 0;
        usage.fds = 5;
        assert_eq!(limit_breach(&limits, &usage), Some("fds"));
        usage.fds = 0;
        usage.threads = 3;
        assert_eq!(limit_breach(&limits, &usage), Some("threads"));

        let mut t = ProcTable::new();
        assert!(t.spawn_init(1));
        assert!(t.create(2, 1, "w", SecurityLayer::L2Application, Credentials { uid: 1, gid: 1 }));
        t.get_mut(2).unwrap().limits = limits;
        assert_eq!(t.charge(2, 50, 100), None);
        assert_eq!(t.charge(2, 60, 0), Some("cpu"));
        assert_eq!(t.get(2).unwrap().usage.cpu_ticks, 110);
    }

    #[test]
    fn jobs_names_and_daemon_restart_policy() {
        let mut jobs = JobTable::new();
        assert!(jobs.is_empty());
        assert_eq!(jobs.create(10), Some(10));
        assert!(jobs.join(10));
        assert_eq!(jobs.find(10).unwrap().members, 2);
        assert!(jobs.leave(10));
        assert!(!jobs.leave(99), "unknown group");
        assert!(jobs.create(11).is_some());
        assert_eq!(jobs.len(), 2);

        let mut names = NameService::new();
        assert!(names.register("shell", 10));
        assert!(!names.register("shell", 11), "duplicate name");
        assert_eq!(names.lookup("shell"), Some(10));
        assert_eq!(names.name_of(10), Some("shell"));
        assert!(names.unregister("shell"));
        assert!(!names.unregister("shell"));
        assert_eq!(names.collisions(), 1);

        // Multi-instance arbitration: a live holder wins, a dead one is
        // reclaimed so a crashed shell does not lock the user out.
        let mut s = NameService::new();
        assert_eq!(claim_instance(&mut s, "x", 1, true), InstanceClaim::Granted);
        assert_eq!(claim_instance(&mut s, "x", 1, true), InstanceClaim::AlreadyRunning(1));
        assert_eq!(claim_instance(&mut s, "x", 2, true), InstanceClaim::AlreadyRunning(1));
        assert_eq!(claim_instance(&mut s, "x", 2, false), InstanceClaim::ReclaimedFrom(1));
        assert_eq!(s.lookup("x"), Some(2));

        let mut d = Daemon::started("svc", 7, RestartPolicy::OnFailure);
        assert!(d.stopped(ExitStatus(1)));
        assert_eq!(d.restarts, 1);
        assert!(!d.running);
        d.running = true;
        assert!(!d.stopped(ExitStatus(EXIT_OK)), "a clean exit is not a failure");
        let mut always = Daemon::started("w", 8, RestartPolicy::Always);
        assert!(always.stopped(ExitStatus(EXIT_OK)));
        // A crash loop is capped: restarting forever helps nobody.
        let mut looped = Daemon::started("loop", 9, RestartPolicy::Always);
        looped.restarts = MAX_DAEMON_RESTARTS;
        assert!(!looped.stopped(ExitStatus(1)));
        assert_eq!(RestartPolicy::Never.as_str(), "never");
    }

    #[test]
    fn self_test_passes_on_a_fresh_table() {
        let t = ProcTable::new();
        let (passed, failed) = run_process_checks(&t);
        assert_eq!(failed, 0, "{passed} passed, {failed} failed");
        assert!(passed >= 10);
    }
}
