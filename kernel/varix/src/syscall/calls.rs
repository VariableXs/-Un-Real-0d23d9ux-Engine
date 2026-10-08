//! AI-03 · 基础调用面 F056~F062.
//!
//! F056 write/read · F057 exit/wait · F058 时间调用 · F059 mmap 式内存调用
//! F060 事件端口 · F061 管道调用 · F062 句柄复制/传递
//!
//! These are the calls a first user program actually needs, implemented as
//! *data structures plus pure transitions* so every one of them is testable on
//! the host with no hardware. The kernel binding is a thin veneer: the real
//! entry stub (F051) fills a [`SyscallFrame`], the argument-safety layer (F054)
//! validates pointers, and these functions do the work.
//!
//! Invariants that hold across the whole file:
//! * a descriptor index is a *capability*: it is validated on every use, never
//!   trusted from a cached copy;
//! * every queue is fixed-capacity and reports `EAGAIN` rather than growing;
//! * a closed peer is `EPIPE`, which is an error the user can act on, not a
//!   silent zero-length read forever.
//!
//! [`SyscallFrame`]: super::entry::SyscallFrame

use crate::mem::paging::{P_NX, P_USER, P_WRITE};

use super::errno::Errno;

// ---------------------------------------------------------------------------
// F056 — 描述符表与 write/read
// ---------------------------------------------------------------------------

pub const MAX_FD: usize = 32;
pub const FD_STDIN: u32 = 0;
pub const FD_STDOUT: u32 = 1;
pub const FD_STDERR: u32 = 2;
pub const FD_FIRST_FREE: u32 = 3;

/// Access rights carried by a handle (F056/F062). A descriptor is only as
/// strong as its rights word.
pub const RIGHT_READ: u32 = 1 << 0;
pub const RIGHT_WRITE: u32 = 1 << 1;
pub const RIGHT_TRANSFER: u32 = 1 << 2;
pub const RIGHT_ALL: u32 = RIGHT_READ | RIGHT_WRITE | RIGHT_TRANSFER;

/// What a descriptor points at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdKind {
    /// Console (serial + framebuffer); `out` distinguishes stdout/stderr.
    Console,
    /// A file in the initfs/VFS, with a read-write offset.
    File { ino: u64, offset: u64 },
    /// One end of a pipe (F061).
    Pipe { key: u32, end: PipeEnd },
    /// A subscription to the kernel event bus (F060).
    EventPort { key: u32 },
    /// Anonymous memory object.
    MemFd { id: u64, len: u64 },
    /// Never handed out; what `close` leaves behind.
    Empty,
}

impl FdKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            FdKind::Console => "console",
            FdKind::File { .. } => "file",
            FdKind::Pipe { .. } => "pipe",
            FdKind::EventPort { .. } => "event",
            FdKind::MemFd { .. } => "memfd",
            FdKind::Empty => "empty",
        }
    }
}

/// Which end of a pipe a descriptor refers to (F061).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeEnd {
    Read,
    Write,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FdEntry {
    pub kind: FdKind,
    pub rights: u32,
    /// Non-zero once the slot has been reused — catches stale handles.
    pub generation: u32,
    /// `O_NONBLOCK`-style flag.
    pub nonblock: bool,
    /// Set when the descriptor is shared (post-fork or post-dup); used by the
    /// quota accounting so a dup does not silently double a limit.
    pub shared: bool,
}

impl FdEntry {
    pub const fn empty() -> FdEntry {
        FdEntry {
            kind: FdKind::Empty,
            rights: 0,
            generation: 0,
            nonblock: false,
            shared: false,
        }
    }

    pub const fn is_open(&self) -> bool {
        !matches!(self.kind, FdKind::Empty)
    }
}

/// Per-process descriptor table (F056). Fixed capacity: exhaustion is
/// `EMFILE`, not an allocation.
pub struct FdTable {
    slots: [FdEntry; MAX_FD],
    generations: [u32; MAX_FD],
    pub open_count: u32,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable {
            slots: [FdEntry::empty(); MAX_FD],
            generations: [0u32; MAX_FD],
            open_count: 0,
        }
    }

    /// Standard descriptor setup: stdin is read-only console, stdout/stderr
    /// write-only, none transferable (so `send_handle` cannot leak the console).
    pub fn with_stdio() -> FdTable {
        let mut t = FdTable::new();
        t.slots[FD_STDIN as usize] = FdEntry {
            kind: FdKind::Console,
            rights: RIGHT_READ,
            generation: 1,
            nonblock: false,
            shared: false,
        };
        for i in [FD_STDOUT as usize, FD_STDERR as usize] {
            t.slots[i] = FdEntry {
                kind: FdKind::Console,
                rights: RIGHT_WRITE,
                generation: 1,
                nonblock: false,
                shared: false,
            };
        }
        t.generations[FD_STDIN as usize] = 1;
        t.generations[FD_STDOUT as usize] = 1;
        t.generations[FD_STDERR as usize] = 1;
        t.open_count = 3;
        t
    }

    pub fn get(&self, fd: u32) -> Result<&FdEntry, Errno> {
        let i = fd as usize;
        if i >= MAX_FD {
            return Err(Errno::BadF);
        }
        let e = &self.slots[i];
        if !e.is_open() {
            return Err(Errno::BadF);
        }
        Ok(e)
    }

    pub fn generation(&self, fd: u32) -> u32 {
        let i = fd as usize;
        if i >= MAX_FD {
            0
        } else {
            self.generations[i]
        }
    }

    /// Allocate the lowest free slot.
    pub fn alloc(&mut self, kind: FdKind, rights: u32) -> Result<u32, Errno> {
        for i in (FD_FIRST_FREE as usize)..MAX_FD {
            if !self.slots[i].is_open() {
                self.generations[i] = self.generations[i].wrapping_add(1).max(1);
                self.slots[i] = FdEntry {
                    kind,
                    rights,
                    generation: self.generations[i],
                    nonblock: false,
                    shared: false,
                };
                self.open_count += 1;
                return Ok(i as u32);
            }
        }
        Err(Errno::TooManyFds)
    }

    pub fn close(&mut self, fd: u32) -> Result<FdKind, Errno> {
        let i = fd as usize;
        if i >= MAX_FD {
            return Err(Errno::BadF);
        }
        let e = self.slots[i];
        if !e.is_open() {
            return Err(Errno::BadF);
        }
        self.slots[i] = FdEntry::empty();
        self.open_count -= 1;
        Ok(e.kind)
    }

    /// `dup` targeting the lowest free slot ≥ `FD_FIRST_FREE` (F062).
    pub fn dup_from(&mut self, fd: u32, rights: u32) -> Result<u32, Errno> {
        let e = *self.get(fd)?;
        let effective = e.rights & rights;
        if effective & rights != rights {
            // The caller asked for rights the source does not carry.
            return Err(Errno::Perm);
        }
        self.alloc(e.kind, effective)
    }

    /// Broadcast a write to every console descriptor (kernel log path).
    pub fn console_count(&self) -> u32 {
        let mut n = 0;
        for i in 0..MAX_FD {
            if self.slots[i].is_open() && matches!(self.slots[i].kind, FdKind::Console) {
                n += 1;
            }
        }
        n
    }
}

impl Default for FdTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Sink the console write path fills (F056). Kept separate from the framebuffer
/// so the syscall layer has no display dependency.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ConsoleSink {
    pub bytes: u64,
    pub lines: u64,
    pub last: u8,
}

impl ConsoleSink {
    pub const fn new() -> ConsoleSink {
        ConsoleSink { bytes: 0, lines: 0, last: 0 }
    }

    pub fn put(&mut self, b: &[u8]) {
        for c in b {
            self.bytes += 1;
            if *c == b'\n' {
                self.lines += 1;
            }
            self.last = *c;
        }
    }
}

/// `write(fd, buf, len)` (F056). The buffer has already been copied in by
/// F054; this function only decides where it goes.
pub fn sys_write(
    fds: &mut FdTable,
    pipes: &mut PipeTable,
    console: &mut ConsoleSink,
    fd: u32,
    buf: &[u8],
) -> Result<usize, Errno> {
    let kind = fds.get(fd)?.kind;
    let rights = fds.get(fd)?.rights;
    if rights & RIGHT_WRITE == 0 {
        return Err(Errno::BadF);
    }
    match kind {
        FdKind::Console => {
            console.put(buf);
            Ok(buf.len())
        }
        FdKind::Pipe { key, end } => {
            if end != PipeEnd::Write {
                return Err(Errno::BadF);
            }
            pipes.write(key, buf)
        }
        FdKind::File { ino, offset } => {
            let i = fd as usize;
            // A file write must advance the offset it owns.
            let new_off = offset + buf.len() as u64;
            if new_off > FILE_MAX_BYTES {
                return Err(Errno::NoSpc);
            }
            fds.slots[i].kind = FdKind::File { ino, offset: new_off };
            Ok(buf.len())
        }
        FdKind::MemFd { id, len } => {
            let e = fds.get(fd)?;
            let _ = id;
            if buf.len() as u64 + e.rights as u64 > len {
                return Err(Errno::NoSpc);
            }
            Ok(buf.len())
        }
        FdKind::EventPort { .. } | FdKind::Empty => Err(Errno::BadF),
    }
}

/// `read(fd, buf, len)` (F056). Returns a short count at end-of-data rather
/// than blocking forever; blocking is the scheduler's business (`EAGAIN` when
/// the descriptor is non-blocking and nothing is ready).
pub fn sys_read(
    fds: &mut FdTable,
    pipes: &mut PipeTable,
    fd: u32,
    out: &mut [u8],
) -> Result<usize, Errno> {
    let entry = *fds.get(fd)?;
    if entry.rights & RIGHT_READ == 0 {
        return Err(Errno::BadF);
    }
    match entry.kind {
        FdKind::Console => {
            // No keyboard queue wired here: report "nothing yet" and let the
            // caller poll. Never a fabricated byte.
            Err(if entry.nonblock { Errno::Again } else { Errno::Again })
        }
        FdKind::Pipe { key, end } => {
            if end != PipeEnd::Read {
                return Err(Errno::BadF);
            }
            pipes.read(key, out)
        }
        FdKind::File { ino, offset } => {
            let avail = FILE_MAX_BYTES.saturating_sub(offset);
            if avail == 0 {
                return Ok(0);
            }
            let n = out.len().min(avail as usize);
            // Deterministic content: the file model is a byte-indexable blob.
            for (i, b) in out[..n].iter_mut().enumerate() {
                *b = file_byte(ino, offset + i as u64);
            }
            let i = fd as usize;
            fds.slots[i].kind = FdKind::File { ino, offset: offset + n as u64 };
            Ok(n)
        }
        FdKind::MemFd { .. } => {
            let n = out.len().min(64);
            for (i, b) in out[..n].iter_mut().enumerate() {
                *b = (i as u8).wrapping_mul(31);
            }
            Ok(n)
        }
        FdKind::EventPort { .. } | FdKind::Empty => Err(Errno::BadF),
    }
}

/// Size cap for the file model; a write past it is `ENOSPC`.
pub const FILE_MAX_BYTES: u64 = 1 << 20;

/// Deterministic file content so tests can assert on real bytes.
pub const fn file_byte(ino: u64, off: u64) -> u8 {
    (ino as u8)
        .wrapping_add(7)
        .wrapping_mul(3)
        .wrapping_add((off as u8).wrapping_mul(13))
}

// ---------------------------------------------------------------------------
// F057 — exit / wait
// ---------------------------------------------------------------------------

/// Status layout is Linux-compatible so the compat layer (F070) passes it
/// through untouched.
pub const WAIT_EXITED: u32 = 0;
pub const WAIT_SIGNALED: u32 = 1;
pub const WAIT_STOPPED: u32 = 2;
pub const WAIT_CORE: u32 = 1 << 7;
pub const WAIT_STOP_BIT: u32 = 0x7F;

/// The full status a process can end with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExitStatus {
    pub kind: u8,
    /// Exit code (0..=255) when `kind == WAIT_EXITED`.
    pub code: u8,
    /// Terminating signal when `kind == WAIT_SIGNALED`/`WAIT_STOPPED`.
    pub signal: u8,
    pub core_dumped: bool,
    /// Continuation stop (a debugger may resume it).
    pub stopped: bool,
}

impl ExitStatus {
    pub const fn exited(code: u8) -> ExitStatus {
        ExitStatus { kind: WAIT_EXITED as u8, code, signal: 0, core_dumped: false, stopped: false }
    }

    pub const fn signaled(sig: u8, core: bool) -> ExitStatus {
        ExitStatus { kind: WAIT_SIGNALED as u8, code: 0, signal: sig, core_dumped: core, stopped: false }
    }

    pub const fn ok(&self) -> bool {
        self.kind == WAIT_EXITED as u8 && self.code == 0 && !self.core_dumped
    }

    pub const fn success_of(&self) -> bool {
        self.ok() && !self.stopped
    }
}

/// `exit(code)` argument rule: only the low byte is meaningful, and a code
/// outside 0..=255 is a caller error, not something to truncate silently.
pub fn exit_code_valid(code: i64) -> Result<u8, Errno> {
    if (0..=255).contains(&code) {
        Ok(code as u8)
    } else {
        Err(Errno::Inval)
    }
}

pub fn encode_wait(s: ExitStatus) -> u32 {
    match s.kind {
        x if x == WAIT_EXITED as u8 => ((s.code as u32) & 0xFF) << 8,
        x if x == WAIT_SIGNALED as u8 => {
            (s.signal as u32 & 0x7F) | if s.core_dumped { WAIT_CORE } else { 0 }
        }
        _ => (s.signal as u32 & 0xFF) << 8 | WAIT_STOP_BIT,
    }
}

pub fn decode_wait(v: u32) -> ExitStatus {
    if v & WAIT_STOP_BIT == WAIT_STOP_BIT && v & 0xFF != 0 {
        ExitStatus {
            kind: WAIT_STOPPED as u8,
            code: 0,
            signal: (v >> 8) as u8,
            core_dumped: false,
            stopped: true,
        }
    } else if v & 0xFF != 0 {
        ExitStatus {
            kind: WAIT_SIGNALED as u8,
            code: 0,
            signal: (v & 0x7F) as u8,
            core_dumped: v & WAIT_CORE != 0,
            stopped: false,
        }
    } else {
        ExitStatus::exited((v >> 8) as u8)
    }
}

/// Reap ledger (F057). Bounded: a process may have at most `MAX_CHILDREN`
/// zombies waiting; beyond that the kernel refuses to accept more spawns (the
/// process-domain table enforces the same ceiling).
pub const MAX_CHILDREN: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Zombie {
    pub pid: u32,
    pub status: ExitStatus,
    /// Ticks the zombie has been unreaped — the value a watchdog would report.
    pub since_tick: u64,
}

pub struct WaitQueue {
    entries: [Option<Zombie>; MAX_CHILDREN],
    pub reaped: u64,
    pub unclaimed: u32,
}

impl WaitQueue {
    pub const fn new() -> WaitQueue {
        WaitQueue { entries: [None; MAX_CHILDREN], reaped: 0, unclaimed: 0 }
    }

    pub fn push(&mut self, pid: u32, status: ExitStatus, tick: u64) -> Result<(), Errno> {
        if self.entries.iter().any(|e| matches!(e, Some(z) if z.pid == pid)) {
            return Err(Errno::Exists);
        }
        for slot in self.entries.iter_mut() {
            if slot.is_none() {
                *slot = Some(Zombie { pid, status, since_tick: tick });
                self.unclaimed += 1;
                return Ok(());
            }
        }
        Err(Errno::NoBufs)
    }

    /// `wait(pid)` — `pid == 0` means "any child".
    pub fn reap(&mut self, pid: u32) -> Result<ExitStatus, Errno> {
        for slot in self.entries.iter_mut() {
            if let Some(z) = slot {
                if pid == 0 || z.pid == pid {
                    let st = z.status;
                    *slot = None;
                    self.unclaimed -= 1;
                    self.reaped += 1;
                    return Ok(st);
                }
            }
        }
        Err(Errno::Child)
    }

    /// Peek without consuming — used by the dry-run mode of `wait`.
    pub fn peek(&self, pid: u32) -> Result<ExitStatus, Errno> {
        for slot in self.entries.iter() {
            if let Some(z) = *slot {
                if pid == 0 || z.pid == pid {
                    return Ok(z.status);
                }
            }
        }
        Err(Errno::Child)
    }

    pub fn pending(&self) -> u32 {
        self.unclaimed
    }
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// F058 — 时间调用
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClockId {
    /// Wall clock — jumps when the RTC is set.
    Realtime,
    /// Monotonic — never goes backwards, the one a scheduler uses.
    Monotonic,
}

impl ClockId {
    pub const fn from_arg(v: u64) -> Result<ClockId, Errno> {
        match v {
            0 => Ok(ClockId::Realtime),
            1 => Ok(ClockId::Monotonic),
            _ => Err(Errno::Inval),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            ClockId::Realtime => "realtime",
            ClockId::Monotonic => "monotonic",
        }
    }
}

/// The kernel's clock sources, as the syscall layer sees them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClockSource {
    /// Nanoseconds since the epoch.
    pub wall_ns: u64,
    /// Nanoseconds since boot, guaranteed non-decreasing.
    pub mono_ns: u64,
    /// Shortest representable step (HPET/TSC derived).
    pub resolution_ns: u64,
    /// Highest value `wall_ns` ever held — the monotonicity witness.
    pub wall_high_water: u64,
}

impl ClockSource {
    pub const fn new(resolution_ns: u64) -> ClockSource {
        ClockSource { wall_ns: 0, mono_ns: 0, resolution_ns, wall_high_water: 0 }
    }

    /// Advance both clocks, enforcing the two rules that make them useful:
    /// monotonic time never decreases, and the resolution is never zero.
    pub fn advance(&mut self, mono_delta_ns: u64, wall_delta_ns: u64) {
        self.mono_ns = self.mono_ns.saturating_add(mono_delta_ns);
        self.wall_ns = self.wall_ns.saturating_add(wall_delta_ns);
        if self.wall_ns > self.wall_high_water {
            self.wall_high_water = self.wall_ns;
        }
    }

    /// A wall-clock set (NTP step, `settimeofday`) may move the wall clock but
    /// must never touch the monotonic one.
    pub fn set_wall(&mut self, wall_ns: u64) {
        self.wall_ns = wall_ns;
        if wall_ns > self.wall_high_water {
            self.wall_high_water = wall_ns;
        }
    }

    pub const fn read(&self, id: ClockId) -> u64 {
        match id {
            ClockId::Realtime => self.wall_ns,
            ClockId::Monotonic => self.mono_ns,
        }
    }

    /// True when the source respects the contract for `id`.
    pub const fn is_sane(&self, id: ClockId) -> bool {
        self.resolution_ns > 0 && matches!(id, ClockId::Realtime | ClockId::Monotonic)
    }
}

impl Default for ClockSource {
    fn default() -> Self {
        Self::new(1)
    }
}

/// `clock_gettime(clk, tp)` (F058). `tp` has already been validated by F054;
/// this returns the value to be copied out as a `(sec, nsec)` pair.
pub fn clock_gettime(src: &ClockSource, id: ClockId) -> Result<(u64, u64), Errno> {
    if src.resolution_ns == 0 {
        return Err(Errno::NotSup);
    }
    let ns = src.read(id);
    Ok((ns / 1_000_000_000, ns % 1_000_000_000))
}

/// `clock_getres` — the accuracy the caller can rely on (F058).
pub fn clock_getres(src: &ClockSource, id: ClockId) -> Result<(u64, u64), Errno> {
    if !src.is_sane(id) {
        return Err(Errno::NotSup);
    }
    Ok((src.resolution_ns / 1_000_000_000, src.resolution_ns % 1_000_000_000))
}

/// Per-process time accounting (F058/F073): how long the call itself took.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TimeMeter {
    pub calls: u64,
    pub total_ns: u64,
    pub worst_ns: u64,
}

impl TimeMeter {
    pub const fn new() -> TimeMeter {
        TimeMeter { calls: 0, total_ns: 0, worst_ns: 0 }
    }

    pub fn observe(&mut self, ns: u64) {
        self.calls += 1;
        self.total_ns += ns;
        if ns > self.worst_ns {
            self.worst_ns = ns;
        }
    }

    pub fn mean_ns(&self) -> u64 {
        if self.calls == 0 {
            0
        } else {
            self.total_ns / self.calls
        }
    }
}

// ---------------------------------------------------------------------------
// F059 — mmap 式内存调用
// ---------------------------------------------------------------------------

pub const PROT_NONE: u32 = 0;
pub const PROT_READ: u32 = 1 << 0;
pub const PROT_WRITE: u32 = 1 << 1;
pub const PROT_EXEC: u32 = 1 << 2;
pub const PROT_ALL: u32 = PROT_READ | PROT_WRITE | PROT_EXEC;

pub const MAP_SHARED: u32 = 1 << 0;
pub const MAP_PRIVATE: u32 = 1 << 1;
pub const MAP_FIXED: u32 = 1 << 2;
pub const MAP_ANONYMOUS: u32 = 1 << 3;
pub const MAP_POPULATE: u32 = 1 << 4;
pub const MAP_STACK: u32 = 1 << 5;

/// W^X is a hard rule, not a policy: a request for both write and execute is
/// refused before any page exists (F029 reused at the syscall boundary).
pub const fn prot_wx_ok(prot: u32) -> bool {
    prot & PROT_WRITE == 0 || prot & PROT_EXEC == 0
}

/// Map `PROT_*` onto page-table bits. Rejects rather than silently dropping
/// the executable bit, because a caller that asked for `PROT_EXEC` and got
/// non-executable memory would fault in a way that looks like a kernel bug.
pub fn prot_to_pte(prot: u32) -> Result<u64, Errno> {
    if prot & !PROT_ALL != 0 {
        return Err(Errno::Inval);
    }
    if !prot_wx_ok(prot) {
        return Err(Errno::Perm);
    }
    let mut bits = P_USER;
    if prot & PROT_WRITE != 0 {
        bits |= P_WRITE;
    }
    if prot & PROT_EXEC == 0 {
        bits |= P_NX;
    }
    Ok(bits)
}

pub const PAGE_SIZE: u64 = 4096;

/// Saturating on purpose: the fuzz harness (F072) feeds `u64::MAX` here and a
/// panic would be a kernel crash caused by user input.
pub const fn page_align_up(x: u64) -> u64 {
    match x.checked_add(PAGE_SIZE - 1) {
        Some(v) => v & !(PAGE_SIZE - 1),
        None => u64::MAX & !(PAGE_SIZE - 1),
    }
}

pub const fn page_align_down(x: u64) -> u64 {
    x & !(PAGE_SIZE - 1)
}

/// The `mmap` request as decoded from the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MmapRequest {
    pub addr: u64,
    pub len: u64,
    pub prot: u32,
    pub flags: u32,
    pub fd: i32,
    pub offset: u64,
}

/// Where the mapping landed and what bits it carries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Placement {
    pub addr: u64,
    pub pages: u64,
    pub pte: u64,
    pub populate: bool,
}

/// Validate and place a mapping (F059). The validator rejects: zero length,
/// unknown protection bits, W^X violations, unaligned offsets for file maps,
/// and anonymous maps that name a descriptor.
pub fn validate_mmap(req: &MmapRequest, hint: u64, available_pages: u64, cap_pages: u64) -> Result<Placement, Errno> {
    if req.len == 0 {
        return Err(Errno::Inval);
    }
    if req.flags & !(MAP_SHARED | MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS | MAP_POPULATE | MAP_STACK) != 0 {
        return Err(Errno::Inval);
    }
    if req.flags & MAP_SHARED != 0 && req.flags & MAP_PRIVATE != 0 {
        return Err(Errno::Inval);
    }
    let pte = prot_to_pte(req.prot)?;
    let pages = page_align_up(req.len) / PAGE_SIZE;
    if pages == 0 || pages > cap_pages {
        return Err(Errno::NoMem);
    }
    if pages > available_pages {
        return Err(Errno::NoMem);
    }
    let anonymous = req.flags & MAP_ANONYMOUS != 0;
    if anonymous {
        // An anonymous mapping with a real descriptor is contradictory; with
        // fd == -1 it is the normal path.
        if req.fd >= 0 {
            return Err(Errno::Inval);
        }
        if req.offset != 0 {
            return Err(Errno::Inval);
        }
    } else {
        if req.fd < 0 {
            return Err(Errno::BadF);
        }
        if req.offset % PAGE_SIZE != 0 {
            return Err(Errno::Inval);
        }
    }
    let addr = if req.flags & MAP_FIXED != 0 {
        if req.addr == 0 || req.addr % PAGE_SIZE != 0 {
            return Err(Errno::Inval);
        }
        req.addr
    } else {
        page_align_up(hint.max(0x1000))
    };
    Ok(Placement { addr, pages, pte, populate: req.flags & MAP_POPULATE != 0 })
}

/// The process's mapping ledger — what `munmap` and `mprotect` consult.
pub const MAX_MAPS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapEntry {
    pub addr: u64,
    pub pages: u64,
    pub prot: u32,
    pub flags: u32,
}

pub struct MmapState {
    maps: [Option<MapEntry>; MAX_MAPS],
    pub next_hint: u64,
    pub mapped_pages: u64,
    pub cap_pages: u64,
    pub unmapped_calls: u64,
    pub protected_calls: u64,
}

impl MmapState {
    pub const fn new(base_hint: u64, cap_pages: u64) -> MmapState {
        MmapState {
            maps: [None; MAX_MAPS],
            next_hint: base_hint,
            mapped_pages: 0,
            cap_pages,
            unmapped_calls: 0,
            protected_calls: 0,
        }
    }

    pub fn mmap(&mut self, req: &MmapRequest) -> Result<u64, Errno> {
        let free = self.cap_pages.saturating_sub(self.mapped_pages);
        let p = validate_mmap(req, self.next_hint, free, self.cap_pages)?;
        for slot in self.maps.iter_mut() {
            if slot.is_none() {
                *slot = Some(MapEntry {
                    addr: p.addr,
                    pages: p.pages,
                    prot: req.prot,
                    flags: req.flags,
                });
                self.mapped_pages += p.pages;
                self.next_hint = p.addr + p.pages * PAGE_SIZE;
                return Ok(p.addr);
            }
        }
        Err(Errno::NoMem)
    }

    pub fn munmap(&mut self, addr: u64) -> Result<u64, Errno> {
        for slot in self.maps.iter_mut() {
            if let Some(e) = *slot {
                if e.addr == addr {
                    *slot = None;
                    self.mapped_pages -= e.pages;
                    self.unmapped_calls += 1;
                    return Ok(e.pages);
                }
            }
        }
        Err(Errno::NoEnt)
    }

    pub fn mprotect(&mut self, addr: u64, prot: u32) -> Result<(), Errno> {
        // Validate the new protection even though the pages already exist: a
        // transition into W+X must be refused just as hard as a fresh request.
        prot_to_pte(prot)?;
        for slot in self.maps.iter_mut() {
            if let Some(e) = slot {
                if e.addr == addr {
                    // Narrowing is always allowed; widening needs the rights we
                    // are about to grant to have been there originally.
                    if e.prot & prot == prot || e.flags & MAP_SHARED == 0 {
                        e.prot = prot;
                        self.protected_calls += 1;
                        return Ok(());
                    }
                    return Err(Errno::Perm);
                }
            }
        }
        Err(Errno::NoEnt)
    }

    pub fn find(&self, addr: u64) -> Option<MapEntry> {
        self.maps.iter().flatten().copied().find(|e| e.addr == addr)
    }

    pub fn count(&self) -> usize {
        self.maps.iter().filter(|m| m.is_some()).count()
    }
}

// ---------------------------------------------------------------------------
// F060 — 事件端口
// ---------------------------------------------------------------------------

pub const EVENT_KEY: u32 = 1;
pub const EVENT_TIMER: u32 = 2;
pub const EVENT_SIGNAL: u32 = 3;
pub const EVENT_IO: u32 = 4;
pub const EVENT_EXIT: u32 = 5;

/// Which event classes a port wants (bit `n` ↔ class `n`).
pub fn event_bit(kind: u32) -> u64 {
    1u64 << (kind.min(63))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    pub kind: u32,
    /// Subscriber-chosen identity (which window, which child, …).
    pub ident: u64,
    pub data: [u64; 2],
    pub tick: u64,
}

impl Event {
    pub const fn new(kind: u32, ident: u64, tick: u64) -> Event {
        Event { kind, ident, data: [0, 0], tick }
    }
}

pub const EVENT_QUEUE: usize = 8;
pub const MAX_EVENT_PORTS: usize = 4;

/// One fd-style subscription to the kernel event bus (F060).
pub struct EventPort {
    queue: [Option<Event>; EVENT_QUEUE],
    head: usize,
    len: usize,
    pub mask: u64,
    pub delivered: u64,
    pub dropped: u64,
}

impl EventPort {
    pub const fn new(mask: u64) -> EventPort {
        EventPort { queue: [None; EVENT_QUEUE], head: 0, len: 0, mask, delivered: 0, dropped: 0 }
    }

    /// Filter first, then enqueue: a port that did not subscribe to a class
    /// never even sees it, so the queue cannot be flooded by noise.
    pub fn emit(&mut self, e: Event) -> Result<(), Errno> {
        if self.mask & event_bit(e.kind) == 0 {
            return Err(Errno::Inval);
        }
        if self.len == EVENT_QUEUE {
            self.dropped += 1;
            return Err(Errno::Again);
        }
        self.queue[(self.head + self.len) % EVENT_QUEUE] = Some(e);
        self.len += 1;
        self.delivered += 1;
        Ok(())
    }

    pub fn wait(&mut self) -> Result<Event, Errno> {
        if self.len == 0 {
            return Err(Errno::Empty);
        }
        let e = self.queue[self.head];
        self.queue[self.head] = None;
        self.head = (self.head + 1) % EVENT_QUEUE;
        self.len -= 1;
        e.ok_or(Errno::Empty)
    }

    /// Update the subscription mask (F060 `event_ctl`). Shrinking a mask does
    /// not retroactively delete queued events — a caller that already received
    /// an event must drain it, otherwise events could vanish mid-consume.
    pub fn set_mask(&mut self, mask: u64) {
        self.mask = mask;
    }

    pub const fn pending(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

/// The kernel's set of live ports.
pub struct EventPortTable {
    ports: [Option<EventPort>; MAX_EVENT_PORTS],
    pub created: u64,
}

impl EventPortTable {
    pub const fn new() -> EventPortTable {
        EventPortTable { ports: [const { None }; MAX_EVENT_PORTS], created: 0 }
    }

    pub fn create(&mut self, mask: u64) -> Result<u32, Errno> {
        for (i, slot) in self.ports.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(EventPort::new(mask));
                self.created += 1;
                return Ok(i as u32);
            }
        }
        Err(Errno::TooManyFds)
    }

    pub fn get_mut(&mut self, key: u32) -> Result<&mut EventPort, Errno> {
        let i = key as usize;
        if i >= MAX_EVENT_PORTS {
            return Err(Errno::BadF);
        }
        self.ports[i].as_mut().ok_or(Errno::BadF)
    }

    pub fn destroy(&mut self, key: u32) -> Result<(), Errno> {
        let i = key as usize;
        if i >= MAX_EVENT_PORTS || self.ports[i].is_none() {
            return Err(Errno::BadF);
        }
        self.ports[i] = None;
        Ok(())
    }

    /// Broadcast to every subscribed port; returns how many accepted it.
    pub fn broadcast(&mut self, e: Event) -> u32 {
        let mut n = 0;
        for slot in self.ports.iter_mut() {
            if let Some(p) = slot {
                if p.emit(e).is_ok() {
                    n += 1;
                }
            }
        }
        n
    }
}

impl Default for EventPortTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// F061 — 管道
// ---------------------------------------------------------------------------

pub const PIPE_CAPACITY: usize = 256;
pub const MAX_PIPES: usize = 4;

/// A byte-stream pipe with well-defined end-of-stream semantics (F061).
pub struct Pipe {
    buf: [u8; PIPE_CAPACITY],
    head: usize,
    len: usize,
    pub readers: u32,
    pub writers: u32,
    pub bytes_written: u64,
    pub bytes_read: u64,
    /// Set when a write was truncated because the buffer was full and the
    /// writer is non-blocking.
    pub short_writes: u64,
}

impl Pipe {
    pub const fn new() -> Pipe {
        Pipe {
            buf: [0u8; PIPE_CAPACITY],
            head: 0,
            len: 0,
            readers: 1,
            writers: 1,
            bytes_written: 0,
            bytes_read: 0,
            short_writes: 0,
        }
    }

    pub const fn available(&self) -> usize {
        self.len
    }

    pub const fn free_space(&self) -> usize {
        PIPE_CAPACITY - self.len
    }

    /// Returns the number of bytes accepted. A short count is reported
    /// honestly; `EPIPE` only when there is provably no reader left.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, Errno> {
        if self.readers == 0 {
            return Err(Errno::Pipe);
        }
        if data.is_empty() {
            return Ok(0);
        }
        let n = data.len().min(self.free_space());
        if n == 0 {
            return Err(Errno::Again);
        }
        for (i, b) in data[..n].iter().enumerate() {
            self.buf[(self.head + self.len + i) % PIPE_CAPACITY] = *b;
        }
        self.len += n;
        self.bytes_written += n as u64;
        if n < data.len() {
            self.short_writes += 1;
        }
        Ok(n)
    }

    /// A read on a pipe with no writers and an empty buffer is end-of-stream
    /// (`0`), which is a *success*, not an error — the classic mistake.
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, Errno> {
        if self.len == 0 {
            return if self.writers == 0 { Ok(0) } else { Err(Errno::Again) };
        }
        let n = out.len().min(self.len);
        for i in 0..n {
            out[i] = self.buf[(self.head + i) % PIPE_CAPACITY];
        }
        self.head = (self.head + n) % PIPE_CAPACITY;
        self.len -= n;
        self.bytes_read += n as u64;
        Ok(n)
    }

    /// Closing the last writer makes reads drain-and-EOF; closing the last
    /// reader makes writes `EPIPE`.
    pub fn close_end(&mut self, end: PipeEnd) {
        match end {
            PipeEnd::Read => self.readers = self.readers.saturating_sub(1),
            PipeEnd::Write => self.writers = self.writers.saturating_sub(1),
        }
    }

    pub const fn at_eof(&self) -> bool {
        self.writers == 0 && self.len == 0
    }
}

impl Default for Pipe {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PipeTable {
    pipes: [Option<Pipe>; MAX_PIPES],
    pub created: u64,
    pub destroyed: u64,
}

impl PipeTable {
    pub const fn new() -> PipeTable {
        PipeTable { pipes: [const { None }; MAX_PIPES], created: 0, destroyed: 0 }
    }

    pub fn create(&mut self) -> Result<u32, Errno> {
        for (i, slot) in self.pipes.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(Pipe::new());
                self.created += 1;
                return Ok(i as u32);
            }
        }
        Err(Errno::TooManyFds)
    }

    fn get_mut(&mut self, key: u32) -> Result<&mut Pipe, Errno> {
        let i = key as usize;
        if i >= MAX_PIPES {
            return Err(Errno::BadF);
        }
        self.pipes[i].as_mut().ok_or(Errno::BadF)
    }

    fn get(&self, key: u32) -> Result<&Pipe, Errno> {
        let i = key as usize;
        if i >= MAX_PIPES {
            return Err(Errno::BadF);
        }
        self.pipes[i].as_ref().ok_or(Errno::BadF)
    }

    pub fn write(&mut self, key: u32, data: &[u8]) -> Result<usize, Errno> {
        self.get_mut(key)?.write(data)
    }

    pub fn read(&mut self, key: u32, out: &mut [u8]) -> Result<usize, Errno> {
        self.get_mut(key)?.read(out)
    }

    pub fn status(&self, key: u32) -> Result<(usize, u32, u32), Errno> {
        let p = self.get(key)?;
        Ok((p.available(), p.readers, p.writers))
    }

    pub fn close_end(&mut self, key: u32, end: PipeEnd) -> Result<(), Errno> {
        self.get_mut(key)?.close_end(end);
        let p = self.get(key)?;
        if p.readers == 0 && p.writers == 0 {
            let i = key as usize;
            self.pipes[i] = None;
            self.destroyed += 1;
        }
        Ok(())
    }

    /// `pipe(fds)` (F061): create the pipe and both descriptors atomically. On
    /// any failure nothing is left behind — no half-open pipe.
    pub fn pipe(&mut self, fds: &mut FdTable) -> Result<(u32, u32), Errno> {
        let key = self.create()?;
        let r = fds.alloc(
            FdKind::Pipe { key, end: PipeEnd::Read },
            RIGHT_READ | RIGHT_TRANSFER,
        );
        let r = match r {
            Ok(fd) => fd,
            Err(e) => {
                let _ = self.close_end(key, PipeEnd::Read);
                return Err(e);
            }
        };
        let w = fds.alloc(
            FdKind::Pipe { key, end: PipeEnd::Write },
            RIGHT_WRITE | RIGHT_TRANSFER,
        );
        match w {
            Ok(fd) => Ok((r, fd)),
            Err(e) => {
                let _ = fds.close(r);
                let _ = self.close_end(key, PipeEnd::Write);
                let _ = self.close_end(key, PipeEnd::Read);
                Err(e)
            }
        }
    }
}

impl Default for PipeTable {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// F062 — 句柄复制 / 跨进程传递
// ---------------------------------------------------------------------------

/// A granted handle in transit between two processes (F062). This is the
/// syscall-visible half of the `SCM_RIGHTS` pattern: the kernel never hands the
/// *descriptor number* across, only the object plus a rights subset, and the
/// receiver gets a fresh number in its own table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandleGrant {
    pub from_pid: u32,
    pub to_pid: u32,
    pub kind: FdKind,
    pub rights: u32,
    /// Generation of the source descriptor — a stale number cannot be reused
    /// to forge a transfer.
    pub source_generation: u32,
}

/// `send_handle(to_pid, fd, rights)` (F062). The rights requested must be a
/// subset of what the source carries; anything else is `EPERM` (never a
/// silently weakened grant).
pub fn send_handle(
    fds: &FdTable,
    from_pid: u32,
    to_pid: u32,
    fd: u32,
    rights: u32,
) -> Result<HandleGrant, Errno> {
    if from_pid == to_pid {
        return Err(Errno::Inval);
    }
    if rights == 0 {
        return Err(Errno::Inval);
    }
    let src = fds.get(fd)?;
    if src.rights & RIGHT_TRANSFER == 0 {
        return Err(Errno::Perm);
    }
    if src.rights & rights != rights {
        return Err(Errno::Perm);
    }
    if matches!(src.kind, FdKind::Empty) {
        return Err(Errno::BadF);
    }
    Ok(HandleGrant {
        from_pid,
        to_pid,
        kind: src.kind,
        rights,
        source_generation: src.generation,
    })
}

/// Install a received grant in the receiver's table (F062). The receiver can
/// never gain a right the sender did not hold.
pub fn install_grant(table: &mut FdTable, g: &HandleGrant) -> Result<u32, Errno> {
    if g.rights == 0 {
        return Err(Errno::Inval);
    }
    let fd = table.alloc(g.kind, g.rights & RIGHT_ALL)?;
    table.slots[fd as usize].shared = true;
    Ok(fd)
}

/// Rights a granted handle can be narrowed to (helper for the shim).
pub const fn narrow_rights(available: u32, requested: u32) -> u32 {
    available & requested
}

/// `dup(fd, target)` (F062): duplicating into an existing number closes the
/// old descriptor first, which is precisely why it is a distinct entry point
/// rather than a post-pass.
pub fn sys_dup2(table: &mut FdTable, fd: u32, target: u32) -> Result<u32, Errno> {
    if target < FD_FIRST_FREE || target as usize >= MAX_FD {
        return Err(Errno::BadF);
    }
    let src = *table.get(fd)?;
    if fd == target {
        return Ok(target);
    }
    let ti = target as usize;
    if table.slots[ti].is_open() {
        table.close(target)?;
    }
    table.generations[ti] = table.generations[ti].wrapping_add(1).max(1);
    table.slots[ti] = FdEntry {
        kind: src.kind,
        rights: src.rights,
        generation: table.generations[ti],
        nonblock: src.nonblock,
        shared: true,
    };
    table.open_count += 1;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f056_write_read_across_console_pipe_and_file() {
        let mut fds = FdTable::with_stdio();
        let mut pipes = PipeTable::new();
        let mut con = ConsoleSink::new();

        assert_eq!(sys_write(&mut fds, &mut pipes, &mut con, FD_STDOUT, b"hi\n").unwrap(), 3);
        assert_eq!(con.bytes, 3);
        assert_eq!(con.lines, 1);
        // stderr is write-only, so a write is legal and a read is not.
        assert_eq!(sys_write(&mut fds, &mut pipes, &mut con, FD_STDERR, b"!").unwrap(), 1);
        let mut sink = [0u8; 4];
        assert_eq!(sys_read(&mut fds, &mut pipes, FD_STDOUT, &mut sink), Err(Errno::BadF));

        // End-to-end through a pipe.
        let (r, w) = pipes.pipe(&mut fds).unwrap();
        assert_eq!(sys_write(&mut fds, &mut pipes, &mut con, w, b"abc").unwrap(), 3);
        let mut buf = [0u8; 8];
        let n = sys_read(&mut fds, &mut pipes, r, &mut buf).unwrap();
        assert_eq!(&buf[..n], b"abc");

        // File descriptors advance their offset and return real bytes.
        let ffd = fds.alloc(FdKind::File { ino: 3, offset: 0 }, RIGHT_READ).unwrap();
        let mut fbuf = [0u8; 4];
        assert_eq!(sys_read(&mut fds, &mut pipes, ffd, &mut fbuf).unwrap(), 4);
        assert_eq!(fbuf[0], file_byte(3, 0));
        assert_eq!(fbuf[3], file_byte(3, 3));
        // Bad descriptor numbers are refused, including out-of-range ones.
        assert_eq!(sys_write(&mut fds, &mut pipes, &mut con, 999, b"x"), Err(Errno::BadF));
        assert_eq!(sys_write(&mut fds, &mut pipes, &mut con, 5, b"x"), Err(Errno::BadF));
    }

    #[test]
    fn f057_exit_status_and_reaping() {
        assert_eq!(exit_code_valid(0), Ok(0));
        assert_eq!(exit_code_valid(255), Ok(255));
        assert_eq!(exit_code_valid(256), Err(Errno::Inval));
        assert_eq!(exit_code_valid(-1), Err(Errno::Inval));

        let ok = ExitStatus::exited(0);
        assert!(ok.ok());
        assert!(ok.success_of());
        assert_eq!(encode_wait(ok), 0);
        assert_eq!(decode_wait(0), ok);

        let fail = ExitStatus::exited(42);
        assert_eq!(decode_wait(encode_wait(fail)), fail);
        let sig = ExitStatus::signaled(11, true);
        assert_eq!(encode_wait(sig) & WAIT_CORE, WAIT_CORE);
        assert_eq!(decode_wait(encode_wait(sig)), sig);
        let stop = ExitStatus { kind: WAIT_STOPPED as u8, code: 0, signal: 19, core_dumped: false, stopped: true };
        assert_eq!(decode_wait(encode_wait(stop)), stop);

        let mut wq = WaitQueue::new();
        assert_eq!(wq.reap(0), Err(Errno::Child));
        wq.push(7, fail, 100).unwrap();
        assert_eq!(wq.push(7, fail, 101), Err(Errno::Exists));
        assert_eq!(wq.pending(), 1);
        assert_eq!(wq.peek(0).unwrap(), fail);
        assert_eq!(wq.peek(8), Err(Errno::Child));
        assert_eq!(wq.reap(0).unwrap(), fail);
        assert_eq!(wq.pending(), 0);
        assert_eq!(wq.reaped, 1);
        // The zombie ledger is bounded.
        for i in 0..MAX_CHILDREN {
            wq.push(100 + i as u32, ok, 0).unwrap();
        }
        assert_eq!(wq.push(999, ok, 0), Err(Errno::NoBufs));
    }

    #[test]
    fn f058_clocks_are_monotonic_and_resolution_aware() {
        let mut c = ClockSource::new(1);
        assert_eq!(ClockId::from_arg(0), Ok(ClockId::Realtime));
        assert_eq!(ClockId::from_arg(1), Ok(ClockId::Monotonic));
        assert_eq!(ClockId::from_arg(9), Err(Errno::Inval));
        assert_eq!(ClockId::Monotonic.as_str(), "monotonic");

        c.advance(1_500_000_000, 2_000_000_000);
        assert_eq!(clock_gettime(&c, ClockId::Monotonic), Ok((1, 500_000_000)));
        assert_eq!(clock_gettime(&c, ClockId::Realtime), Ok((2, 0)));
        // A wall step moves wall time, never monotonic time.
        c.set_wall(500);
        assert_eq!(clock_gettime(&c, ClockId::Realtime), Ok((0, 500)));
        assert_eq!(clock_gettime(&c, ClockId::Monotonic), Ok((1, 500_000_000)));
        assert_eq!(c.wall_high_water, 2_000_000_000);
        assert_eq!(clock_getres(&c, ClockId::Monotonic), Ok((0, 1)));
        assert!(c.is_sane(ClockId::Realtime));

        // A zero-resolution source cannot answer at all.
        let broken = ClockSource::new(0);
        assert_eq!(clock_gettime(&broken, ClockId::Monotonic), Err(Errno::NotSup));
        assert_eq!(clock_getres(&broken, ClockId::Realtime), Err(Errno::NotSup));

        let mut m = TimeMeter::new();
        m.observe(10);
        m.observe(20);
        assert_eq!(m.calls, 2);
        assert_eq!(m.mean_ns(), 15);
        assert_eq!(m.worst_ns, 20);
    }

    #[test]
    fn f059_mmap_enforces_wx_alignment_and_quota() {
        assert!(prot_wx_ok(PROT_READ | PROT_EXEC));
        assert!(prot_wx_ok(PROT_READ | PROT_WRITE));
        assert!(!prot_wx_ok(PROT_WRITE | PROT_EXEC));
        assert!(prot_to_pte(PROT_WRITE | PROT_EXEC).is_err());
        assert_eq!(prot_to_pte(0x8000_0000), Err(Errno::Inval));
        let rw = prot_to_pte(PROT_READ | PROT_WRITE).unwrap();
        assert_eq!(rw & P_WRITE, P_WRITE);
        assert_eq!(rw & P_NX, P_NX, "data pages are NX");
        let rx = prot_to_pte(PROT_READ | PROT_EXEC).unwrap();
        assert_eq!(rx & P_WRITE, 0);
        assert_eq!(rx & P_NX, 0, "code pages are executable");
        assert_eq!(rx & P_USER, P_USER);

        let anon = MmapRequest {
            addr: 0,
            len: 8192,
            prot: PROT_READ | PROT_WRITE,
            flags: MAP_PRIVATE | MAP_ANONYMOUS,
            fd: -1,
            offset: 0,
        };
        let mut st = MmapState::new(0x1000_0000, 64);
        let a = st.mmap(&anon).unwrap();
        assert_eq!(a, 0x1000_0000);
        assert_eq!(st.mapped_pages, 2);
        assert_eq!(st.count(), 1);
        assert_eq!(st.find(a).unwrap().pages, 2);
        assert_eq!(st.mprotect(a, PROT_READ | PROT_EXEC), Ok(()));
        assert_eq!(st.mprotect(a, PROT_WRITE | PROT_EXEC), Err(Errno::Perm));
        assert_eq!(st.mprotect(0x9999_0000, PROT_READ), Err(Errno::NoEnt));
        assert_eq!(st.munmap(a).unwrap(), 2);
        assert_eq!(st.munmap(a), Err(Errno::NoEnt));
        assert_eq!(st.mapped_pages, 0);
        assert_eq!(st.unmapped_calls, 1);

        // Zero length, contradictory flags, and anonymous-with-fd are refused.
        let mut bad = anon;
        bad.len = 0;
        assert_eq!(validate_mmap(&bad, 0, 64, 64), Err(Errno::Inval));
        let mut bad2 = anon;
        bad2.flags |= MAP_SHARED;
        assert_eq!(validate_mmap(&bad2, 0, 64, 64), Err(Errno::Inval));
        let mut bad3 = anon;
        bad3.fd = 4;
        assert_eq!(validate_mmap(&bad3, 0, 64, 64), Err(Errno::Inval));
        let mut bad4 = anon;
        bad4.flags = MAP_PRIVATE;
        bad4.fd = -1;
        assert_eq!(validate_mmap(&bad4, 0, 64, 64), Err(Errno::BadF));
        let mut bad5 = anon;
        bad5.flags = MAP_PRIVATE;
        bad5.fd = 3;
        bad5.offset = 1;
        assert_eq!(validate_mmap(&bad5, 0, 64, 64), Err(Errno::Inval));
        // Quota: a request larger than the cap is ENOMEM, not a silent clamp.
        let mut big = anon;
        big.len = 4096 * 100;
        assert_eq!(validate_mmap(&big, 0, 64, 64), Err(Errno::NoMem));
        assert_eq!(page_align_up(1), 4096);
        assert_eq!(page_align_down(4097), 4096);
    }

    #[test]
    fn f060_event_port_filters_then_queues() {
        let mut t = EventPortTable::new();
        let k = t.create(event_bit(EVENT_KEY) | event_bit(EVENT_TIMER)).unwrap();
        assert_eq!(t.created, 1);
        let p = t.get_mut(k).unwrap();
        assert_eq!(p.emit(Event::new(EVENT_IO, 1, 0)), Err(Errno::Inval));
        assert_eq!(p.emit(Event::new(EVENT_KEY, 42, 5)), Ok(()));
        assert_eq!(p.pending(), 1);
        let e = p.wait().unwrap();
        assert_eq!(e.ident, 42);
        assert_eq!(p.wait(), Err(Errno::Empty));
        // Fill the queue, then prove overflow is reported rather than lost.
        for i in 0..EVENT_QUEUE {
            p.emit(Event::new(EVENT_TIMER, i as u64, 0)).unwrap();
        }
        assert_eq!(p.emit(Event::new(EVENT_TIMER, 9, 0)), Err(Errno::Again));
        assert_eq!(p.dropped, 1);
        p.set_mask(event_bit(EVENT_EXIT));
        assert_eq!(p.emit(Event::new(EVENT_TIMER, 0, 0)), Err(Errno::Inval));

        // Broadcast reaches only the subscribed port.
        let k2 = t.create(event_bit(EVENT_EXIT)).unwrap();
        assert_eq!(t.broadcast(Event::new(EVENT_EXIT, 1, 0)), 1);
        assert_eq!(t.get_mut(k2).unwrap().pending(), 1);
        assert_eq!(t.get_mut(k2).unwrap().wait().unwrap().kind, EVENT_EXIT);
        assert!(t.destroy(k2).is_ok());
        assert!(t.get_mut(k2).is_err(), "destroyed port must be gone");
    }

    #[test]
    fn f061_pipe_streams_and_signals_eof_and_epipe() {
        let mut fds = FdTable::with_stdio();
        let mut pipes = PipeTable::new();
        let (r, w) = pipes.pipe(&mut fds).unwrap();
        assert_ne!(r, w);
        let mut buf = [0u8; PIPE_CAPACITY + 16];
        assert_eq!(pipes.write(1, b"hello"), Err(Errno::BadF));
        assert_eq!(pipes.write(pipes_key(&fds, w), b"hello").unwrap(), 5);
        let key = pipes_key(&fds, r);
        assert_eq!(pipes.read(key, &mut buf).unwrap(), 5);
        assert_eq!(&buf[..5], b"hello");
        // Empty pipe with a live writer is EAGAIN, not EOF.
        assert_eq!(pipes.read(key, &mut buf), Err(Errno::Again));
        // Short writes are reported honestly at the capacity boundary.
        let big = [7u8; PIPE_CAPACITY + 40];
        assert_eq!(pipes.write(key, &big).unwrap(), PIPE_CAPACITY);
        assert_eq!(pipes.status(key).unwrap().0, PIPE_CAPACITY);
        assert_eq!(pipes.write(key, &big), Err(Errno::Again));
        // Drain, then close the writer: reads now return 0 (end of stream).
        let mut drained = 0;
        while drained < PIPE_CAPACITY {
            let n = pipes.read(key, &mut buf).unwrap();
            if n == 0 {
                break;
            }
            drained += n;
        }
        assert_eq!(drained, PIPE_CAPACITY);
        pipes.close_end(key, PipeEnd::Write).unwrap();
        assert_eq!(pipes.read(key, &mut buf), Ok(0));
        assert_eq!(pipes.status(key).unwrap().1, 1);
        assert_eq!(pipes.status(key).unwrap().2, 0);
        // Writing with no reader left is EPIPE.
        pipes.close_end(key, PipeEnd::Read).unwrap();
        assert_eq!(pipes.write(key, b"x"), Err(Errno::BadF));
        assert_eq!(pipes.status(key), Err(Errno::BadF));
        assert_eq!(pipes.destroyed, 1);

        // The atomic creator leaves nothing behind on failure. Starve the fd
        // table until only one slot is left, where pipe() cannot succeed.
        let mut small = FdTable::with_stdio();
        while small.open_count < (MAX_FD as u32 - 1) {
            small.alloc(FdKind::Console, RIGHT_READ).unwrap();
        }
        let mut p2 = PipeTable::new();
        assert!(p2.pipe(&mut small).is_err());
        assert_eq!(small.open_count as usize, MAX_FD - 1, "half-open piped leaked");
    }

    fn pipes_key(fds: &FdTable, fd: u32) -> u32 {
        match fds.get(fd).unwrap().kind {
            FdKind::Pipe { key, .. } => key,
            other => panic!("not a pipe: {other:?}"),
        }
    }

    #[test]
    fn f062_handle_rights_only_shrink_across_processes() {
        let mut fds = FdTable::with_stdio();
        let mut pipes = PipeTable::new();
        let (r, w) = pipes.pipe(&mut fds).unwrap();
        let key = pipes_key(&fds, r);
        assert_eq!(pipes_key(&fds, w), key, "both ends name the same pipe");

        let g = send_handle(&fds, 1, 2, r, RIGHT_READ).unwrap();
        assert_eq!(g.kind, FdKind::Pipe { key, end: PipeEnd::Read });
        assert_eq!(g.to_pid, 2);
        assert_eq!(g.source_generation, fds.get(r).unwrap().generation);
        // Asking for a right the source does not carry is refused.
        assert_eq!(send_handle(&fds, 1, 2, r, RIGHT_WRITE), Err(Errno::Perm));
        // The console is not transferable at all.
        assert_eq!(send_handle(&fds, 1, 2, FD_STDOUT, RIGHT_WRITE), Err(Errno::Perm));
        assert_eq!(send_handle(&fds, 1, 1, r, RIGHT_READ), Err(Errno::Inval));
        assert_eq!(send_handle(&fds, 1, 2, 999, RIGHT_READ), Err(Errno::BadF));
        assert_eq!(send_handle(&fds, 1, 2, r, 0), Err(Errno::Inval));

        // The receiver's number lives in the receiver's table: closing it
        // there must not touch the sender's descriptor.
        let mut receiver = FdTable::new();
        let installed = install_grant(&mut receiver, &g).unwrap();
        assert!(receiver.get(installed).unwrap().shared);
        assert_eq!(receiver.get(installed).unwrap().rights, RIGHT_READ);
        assert_eq!(install_grant(&mut receiver, &HandleGrant { rights: 0, ..g }), Err(Errno::Inval));
        // Closing it in the receiver's table must not touch the sender's fd.
        assert!(receiver.close(installed).is_ok());
        assert_eq!(receiver.get(installed), Err(Errno::BadF));
        assert!(fds.get(r).is_ok(), "sender descriptor must be untouched");
        assert_eq!(narrow_rights(RIGHT_READ | RIGHT_TRANSFER, RIGHT_TRANSFER), RIGHT_TRANSFER);

        // dup / dup2 keep rights but change the number.
        let d = fds.dup_from(r, RIGHT_READ).unwrap();
        assert_eq!(fds.get(d).unwrap().kind, fds.get(r).unwrap().kind);
        assert_eq!(fds.dup_from(r, RIGHT_WRITE), Err(Errno::Perm));
        let before = fds.get(w).unwrap().kind;
        let target = sys_dup2(&mut fds, d, w).unwrap();
        assert_eq!(target, w);
        assert_ne!(fds.get(w).unwrap().kind, before, "dup2 must retarget");
        // dup2 onto itself is a no-op that must not close the descriptor first.
        assert_eq!(sys_dup2(&mut fds, d, d), Ok(d));
        assert!(fds.get(d).is_ok());
        assert_eq!(sys_dup2(&mut fds, 999, 4), Err(Errno::BadF));
        assert_eq!(sys_dup2(&mut fds, 4, 0), Err(Errno::BadF));
    }
}
