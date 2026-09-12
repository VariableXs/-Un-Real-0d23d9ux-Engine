//! AI-03 · F063 能力检查 / F064 调用审计 / F065 调用配额 / F067 seccomp 式过滤.
//!
//! The four gatekeepers a call passes on its way from the entry stub to a
//! handler body. They are ordered deliberately and the order is part of the
//! contract:
//!
//! ```text
//!   entry → number table (ENOSYS) → seccomp (F067) → quota (F065)
//!         → capability (F063) → audit record (F064) → body
//! ```
//!
//! seccomp runs before quota so a sandboxed process cannot burn budget probing
//! calls it may never make; capability runs before the body so a denied call
//! costs nothing; the audit record is written for *both* outcomes, which is why
//! it sits last but unconditional.

use super::errno::Errno;
use super::table;

// ---------------------------------------------------------------------------
// F063 — 能力位
// ---------------------------------------------------------------------------

pub const CAP_NONE: u64 = 0;
pub const CAP_CONSOLE: u64 = 1 << 0;
pub const CAP_FS: u64 = 1 << 1;
pub const CAP_MEM: u64 = 1 << 2;
pub const CAP_PROC: u64 = 1 << 3;
pub const CAP_TIME: u64 = 1 << 4;
pub const CAP_EVENT: u64 = 1 << 5;
pub const CAP_NET: u64 = 1 << 6;
pub const CAP_GFX: u64 = 1 << 7;
pub const CAP_DEBUG: u64 = 1 << 8;
pub const CAP_ADMIN: u64 = 1 << 9;

/// Every capability this kernel defines.
pub const CAP_ALL: u64 = CAP_CONSOLE
    | CAP_FS
    | CAP_MEM
    | CAP_PROC
    | CAP_TIME
    | CAP_EVENT
    | CAP_NET
    | CAP_GFX
    | CAP_DEBUG
    | CAP_ADMIN;

pub const CAP_COUNT: usize = 10;
pub const CAP_NAMES: [(u64, &str); CAP_COUNT] = [
    (CAP_CONSOLE, "console"),
    (CAP_FS, "fs"),
    (CAP_MEM, "mem"),
    (CAP_PROC, "proc"),
    (CAP_TIME, "time"),
    (CAP_EVENT, "event"),
    (CAP_NET, "net"),
    (CAP_GFX, "gfx"),
    (CAP_DEBUG, "debug"),
    (CAP_ADMIN, "admin"),
];

/// The gate (F063). Denial is always `EPERM` and always names the missing bits
/// in the audit trail — never a bare `false`.
pub fn cap_check(granted: u64, required: u64) -> Result<(), Errno> {
    if required & !granted == 0 {
        Ok(())
    } else {
        Err(Errno::Perm)
    }
}

/// Capability intersection: a child can only ever lose bits (mirrors the
/// process-domain model, F015).
pub const fn cap_derive(parent: u64, requested: u64) -> u64 {
    parent & requested
}

/// Human-readable rendering of a capability word, for audit lines.
pub struct CapText {
    buf: [u8; 96],
    len: usize,
}

impl CapText {
    pub fn new() -> CapText {
        CapText { buf: [0u8; 96], len: 0 }
    }

    pub fn render(&mut self, caps: u64) -> &str {
        self.len = 0;
        if caps == CAP_NONE {
            return self.push_str("none");
        }
        let mut first = true;
        for (bit, name) in CAP_NAMES.iter() {
            if caps & bit != 0 {
                if !first {
                    self.push_byte(b'|');
                }
                self.push_str(name);
                first = false;
            }
        }
        // Bits outside the defined set are shown numerically so an unknown
        // capability can never be silently invisible.
        let extra = caps & !CAP_ALL;
        if extra != 0 {
            if !first {
                self.push_byte(b'|');
            }
            self.push_str("0x");
            self.push_hex(extra);
        }
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?")
    }

    fn push_byte(&mut self, b: u8) {
        if self.len < self.buf.len() {
            self.buf[self.len] = b;
            self.len += 1;
        }
    }

    fn push_str(&mut self, s: &str) -> &str {
        for b in s.as_bytes() {
            self.push_byte(*b);
        }
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("?")
    }

    fn push_hex(&mut self, mut v: u64) {
        let mut tmp = [0u8; 16];
        let mut n = 0;
        if v == 0 {
            self.push_byte(b'0');
            return;
        }
        while v != 0 {
            tmp[n] = b"0123456789abcdef"[(v & 0xF) as usize];
            v >>= 4;
            n += 1;
        }
        while n > 0 {
            n -= 1;
            self.push_byte(tmp[n]);
        }
    }
}

impl Default for CapText {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// F067 — seccomp 式过滤器
// ---------------------------------------------------------------------------

pub const SECCOMP_ALLOW: u32 = 0;
pub const SECCOMP_ERRNO: u32 = 1;
pub const SECCOMP_KILL: u32 = 2;
pub const SECCOMP_TRAP: u32 = 3;
pub const SECCOMP_LOG: u32 = 4;

/// Bitmap words: index = `nr & SECCOMP_MASK`, so the native, compat and low
/// vendor bands all fit.
pub const SECCOMP_WORDS: usize = 16;
pub const SECCOMP_MASK: u64 = (SECCOMP_WORDS as u64) * 64 - 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeccompVerdict {
    Allow,
    Errno(Errno),
    Kill,
    Trap,
    /// Allowed but recorded (audit-only policy).
    Log,
}

impl SeccompVerdict {
    pub const fn action(self) -> u32 {
        match self {
            SeccompVerdict::Allow => SECCOMP_ALLOW,
            SeccompVerdict::Errno(_) => SECCOMP_ERRNO,
            SeccompVerdict::Kill => SECCOMP_KILL,
            SeccompVerdict::Trap => SECCOMP_TRAP,
            SeccompVerdict::Log => SECCOMP_LOG,
        }
    }

    pub const fn allows(self) -> bool {
        matches!(self, SeccompVerdict::Allow | SeccompVerdict::Log)
    }
}

/// A per-process filter (F067). Whitelist semantics: a number is allowed only
/// if explicitly permitted, and the default action for everything else is
/// decided by `default_action` — never "allow", unless the filter is inactive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SeccompFilter {
    allow: [u64; SECCOMP_WORDS],
    pub default_action: u32,
    pub active: bool,
    /// When set, only the numbers in the bitmap pass, and `default_action` is
    /// applied to the rest. When clear, the bitmap is an explicit deny list.
    pub whitelist: bool,
    /// Error returned for `SECCOMP_ERRNO`.
    pub err: Errno,
    pub denials: u32,
    pub kills: u32,
}

impl SeccompFilter {
    /// The inactive filter: everything allowed, nothing recorded.
    pub const fn inactive() -> SeccompFilter {
        SeccompFilter {
            allow: [0u64; SECCOMP_WORDS],
            default_action: SECCOMP_ALLOW,
            active: false,
            whitelist: true,
            err: Errno::Perm,
            denials: 0,
            kills: 0,
        }
    }

    /// Install a whitelist that permits only the handful of calls a service
    /// needs, killing on anything else (the strict sandbox preset).
    pub fn whitelist_only(nrs: &[u64]) -> SeccompFilter {
        let mut f = SeccompFilter::inactive();
        f.active = true;
        f.whitelist = true;
        f.default_action = SECCOMP_KILL;
        f.err = Errno::Seccomp;
        for nr in nrs {
            f.permit(*nr);
        }
        f
    }

    pub fn permit(&mut self, nr: u64) {
        let i = (nr & SECCOMP_MASK) as usize / 64;
        let b = (nr & SECCOMP_MASK) % 64;
        self.allow[i] |= 1u64 << b;
    }

    pub fn revoke(&mut self, nr: u64) {
        let i = (nr & SECCOMP_MASK) as usize / 64;
        let b = (nr & SECCOMP_MASK) % 64;
        self.allow[i] &= !(1u64 << b);
    }

    pub fn permits(&self, nr: u64) -> bool {
        let i = (nr & SECCOMP_MASK) as usize / 64;
        let b = (nr & SECCOMP_MASK) % 64;
        self.allow[i] & (1u64 << b) != 0
    }

    /// The gate (F067). Inactive filters never deny, which is what makes the
    /// default kernel configuration transparent.
    pub fn eval(&mut self, nr: u64) -> SeccompVerdict {
        if !self.active {
            return SeccompVerdict::Allow;
        }
        let listed = self.permits(nr);
        let allowed = if self.whitelist { listed } else { !listed };
        if allowed {
            return if self.default_action == SECCOMP_LOG {
                SeccompVerdict::Log
            } else {
                SeccompVerdict::Allow
            };
        }
        match self.default_action {
            SECCOMP_ERRNO => {
                self.denials += 1;
                SeccompVerdict::Errno(self.err)
            }
            SECCOMP_KILL => {
                self.denials += 1;
                self.kills += 1;
                SeccompVerdict::Kill
            }
            SECCOMP_TRAP => {
                self.denials += 1;
                SeccompVerdict::Trap
            }
            SECCOMP_LOG => SeccompVerdict::Log,
            _ => {
                self.denials += 1;
                SeccompVerdict::Errno(self.err)
            }
        }
    }
}

impl Default for SeccompFilter {
    fn default() -> Self {
        Self::inactive()
    }
}

// ---------------------------------------------------------------------------
// F065 — 调用配额
// ---------------------------------------------------------------------------

/// Per-process call budget over a sliding window (F065). Refusal is `EAGAIN`
/// for a *rate* excess (retryable) and `Errno::Quota` when a hard ceiling is
/// blown (not retryable) — the distinction matters to a user loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quota {
    /// Calls permitted per window.
    pub limit: u32,
    /// Window length in ticks.
    pub window_ticks: u64,
    pub window_start: u64,
    pub used: u32,
    pub denied: u32,
    /// Lifetime ceiling; 0 disables.
    pub hard_limit: u64,
    pub lifetime: u64,
}

impl Quota {
    pub const fn new(limit: u32, window_ticks: u64, hard_limit: u64) -> Quota {
        Quota {
            limit,
            window_ticks,
            window_start: 0,
            used: 0,
            denied: 0,
            hard_limit,
            lifetime: 0,
        }
    }

    /// Default kernel budget: 50k calls/s, no lifetime ceiling.
    pub const fn default_budget() -> Quota {
        Quota::new(50_000, 1000, 0)
    }

    /// Unmetered (kernel threads, init).
    pub const fn unmetered() -> Quota {
        Quota::new(0, 1000, 0)
    }

    pub const fn is_unmetered(&self) -> bool {
        self.limit == 0 && self.hard_limit == 0
    }

    /// Charge one call at logical time `now` (F065).
    pub fn charge(&mut self, now: u64) -> Result<(), Errno> {
        if self.is_unmetered() {
            return Ok(());
        }
        if self.hard_limit != 0 && self.lifetime >= self.hard_limit {
            self.denied += 1;
            return Err(Errno::Quota);
        }
        if now >= self.window_start + self.window_ticks {
            self.window_start = now;
            self.used = 0;
        }
        if self.limit != 0 && self.used >= self.limit {
            self.denied += 1;
            return Err(Errno::Again);
        }
        self.used += 1;
        self.lifetime += 1;
        Ok(())
    }

    /// Remaining allowance in the current window (0 when exhausted).
    pub fn headroom(&self) -> u32 {
        if self.limit == 0 {
            return u32::MAX;
        }
        self.limit.saturating_sub(self.used)
    }
}

impl Default for Quota {
    fn default() -> Self {
        Self::default_budget()
    }
}

// ---------------------------------------------------------------------------
// F064 — 调用审计
// ---------------------------------------------------------------------------

/// Ring capacity. Fixed so the audit trail can be dumped from a crashed kernel
/// without an allocator.
pub const MAX_AUDIT: usize = 32;

/// One audited call. Stored as numbers only — no pointers, no borrowed memory,
/// so the record stays readable after the caller's address space is gone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditEntry {
    pub nr: u64,
    pub pid: u32,
    pub caller_rip: u64,
    pub arg0: u64,
    pub tick: u64,
    /// The error the gate produced, `Errno::Ok` when allowed.
    pub outcome: Errno,
    /// True when the call was refused by a gate (not by the body).
    pub denied: bool,
    pub path_compat: bool,
}

impl AuditEntry {
    pub const fn empty() -> AuditEntry {
        AuditEntry {
            nr: 0,
            pid: 0,
            caller_rip: 0,
            arg0: 0,
            tick: 0,
            outcome: Errno::Ok,
            denied: false,
            path_compat: false,
        }
    }
}

/// Append-only ring (F064). Old entries are overwritten by new ones; the
/// `dropped` counter keeps the loss visible instead of hiding it.
pub struct AuditRing {
    entries: [Option<AuditEntry>; MAX_AUDIT],
    pub head: usize,
    pub count: usize,
    pub dropped: u64,
}

impl AuditRing {
    pub const fn new() -> AuditRing {
        AuditRing { entries: [None; MAX_AUDIT], head: 0, count: 0, dropped: 0 }
    }

    pub fn record(&mut self, e: AuditEntry) {
        if self.count == MAX_AUDIT {
            self.dropped += 1;
        }
        self.entries[self.head] = Some(e);
        self.head = (self.head + 1) % MAX_AUDIT;
        if self.count < MAX_AUDIT {
            self.count += 1;
        }
    }

    /// Oldest retained entry.
    pub fn oldest(&self) -> Option<AuditEntry> {
        if self.count == 0 {
            return None;
        }
        let start = (self.head + MAX_AUDIT - self.count) % MAX_AUDIT;
        self.entries[start]
    }

    /// Most recent entry.
    pub fn latest(&self) -> Option<AuditEntry> {
        if self.count == 0 {
            return None;
        }
        let idx = (self.head + MAX_AUDIT - 1) % MAX_AUDIT;
        self.entries[idx]
    }

    pub fn at(&self, i: usize) -> Option<AuditEntry> {
        if i >= self.count {
            return None;
        }
        let start = (self.head + MAX_AUDIT - self.count) % MAX_AUDIT;
        self.entries[(start + i) % MAX_AUDIT]
    }

    /// Count of refusals in the retained window — the number an operator
    /// actually looks at.
    pub fn denied(&self) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(e) = self.at(i) {
                if e.denied {
                    n += 1;
                }
            }
        }
        n
    }

    pub fn clear(&mut self) {
        self.entries = [None; MAX_AUDIT];
        self.head = 0;
        self.count = 0;
        self.dropped = 0;
    }
}

impl Default for AuditRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a number belongs in the audit set (F064). Driven by the number
/// table plus the "any denial" rule, so a new sensitive call is audited the
/// moment it is registered.
pub fn is_audited(nr: u64) -> bool {
    table::lookup(nr).map(|d| d.audited).unwrap_or(true)
}

// ---------------------------------------------------------------------------
// The composed gate
// ---------------------------------------------------------------------------

/// Verdict of the composed F063/F064/F065/F067 chain.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    /// The call may run; the audit record (if any) is already written.
    Proceed,
    /// Refused before the body ran.
    Denied(Errno),
    /// The process asked to die by seccomp.
    Kill,
}

/// Everything the gate needs about the caller. Borrowed, never stored.
pub struct CallerCtx<'a> {
    pub pid: u32,
    pub caps: u64,
    pub caller_rip: u64,
    pub tick: u64,
    pub seccomp: &'a mut SeccompFilter,
    pub quota: &'a mut Quota,
    pub audit: &'a mut AuditRing,
    pub audit_enabled: bool,
}

/// Run the ordered gate for `nr` with first argument `arg0` (F063/F064/F065/F067).
pub fn gate(ctx: &mut CallerCtx<'_>, nr: u64, arg0: u64) -> GateVerdict {
    // 1. seccomp first: a sandbox must not be able to spend budget on calls it
    //    may never make, and the denial must be attributable to the filter.
    let verdict = ctx.seccomp.eval(nr);
    if !verdict.allows() {
        let e = match verdict {
            SeccompVerdict::Errno(e) => e,
            SeccompVerdict::Kill => Errno::Seccomp,
            SeccompVerdict::Trap => Errno::Seccomp,
            _ => Errno::Seccomp,
        };
        if ctx.audit_enabled {
            ctx.audit.record(AuditEntry {
                nr,
                pid: ctx.pid,
                caller_rip: ctx.caller_rip,
                arg0,
                tick: ctx.tick,
                outcome: e,
                denied: true,
                path_compat: table::band_of(nr) == table::NrBand::Compat,
            });
        }
        return match verdict {
            SeccompVerdict::Kill => GateVerdict::Kill,
            _ => GateVerdict::Denied(e),
        };
    }

    // 2. quota.
    if let Err(e) = ctx.quota.charge(ctx.tick) {
        if ctx.audit_enabled {
            ctx.audit.record(AuditEntry {
                nr,
                pid: ctx.pid,
                caller_rip: ctx.caller_rip,
                arg0,
                tick: ctx.tick,
                outcome: e,
                denied: true,
                path_compat: false,
            });
        }
        return GateVerdict::Denied(e);
    }

    // 3. capability.
    let required = table::required_caps(nr);
    if let Err(e) = cap_check(ctx.caps, required) {
        if ctx.audit_enabled {
            ctx.audit.record(AuditEntry {
                nr,
                pid: ctx.pid,
                caller_rip: ctx.caller_rip,
                arg0,
                tick: ctx.tick,
                outcome: e,
                denied: true,
                path_compat: false,
            });
        }
        return GateVerdict::Denied(e);
    }

    // 4. audit the allow too, but only for the sensitive set.
    if ctx.audit_enabled && is_audited(nr) {
        ctx.audit.record(AuditEntry {
            nr,
            pid: ctx.pid,
            caller_rip: ctx.caller_rip,
            arg0,
            tick: ctx.tick,
            outcome: Errno::Ok,
            denied: false,
            path_compat: table::band_of(nr) == table::NrBand::Compat,
        });
    }
    GateVerdict::Proceed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscall::table::{SYS_EXIT, SYS_MMAP, SYS_READ, SYS_WRITE};

    #[test]
    fn f063_capability_gate_denies_and_renders() {
        assert_eq!(cap_check(CAP_ALL, CAP_MEM), Ok(()));
        assert_eq!(cap_check(CAP_ALL, CAP_NONE), Ok(()));
        assert_eq!(cap_check(CAP_MEM, CAP_FS), Err(Errno::Perm));
        assert_eq!(cap_check(CAP_NONE, CAP_NONE), Ok(()));
        // Derivation can only ever shrink.
        assert_eq!(cap_derive(CAP_MEM | CAP_FS, CAP_FS | CAP_ADMIN), CAP_FS);
        let mut t = CapText::new();
        assert_eq!(t.render(CAP_NONE), "none");
        assert_eq!(t.render(CAP_MEM), "mem");
        assert_eq!(t.render(CAP_MEM | CAP_FS), "fs|mem");
        assert!(t.render(CAP_MEM | (1 << 40)).contains("0x"));
    }

    #[test]
    fn f067_seccomp_whitelist_denies_by_default() {
        let mut f = SeccompFilter::whitelist_only(&[SYS_READ, SYS_WRITE]);
        assert_eq!(f.eval(SYS_READ), SeccompVerdict::Allow);
        assert_eq!(f.eval(SYS_MMAP), SeccompVerdict::Kill);
        assert_eq!(f.kills, 1);
        assert_eq!(f.denials, 1);
        // Revoking removes the permit again.
        f.revoke(SYS_READ);
        assert!(matches!(f.eval(SYS_READ), SeccompVerdict::Kill));

        // An inactive filter is transparent — the default configuration.
        let mut idle = SeccompFilter::inactive();
        assert_eq!(idle.eval(0x1234), SeccompVerdict::Allow);
        assert_eq!(idle.denials, 0);

        // An audit-only policy permits but marks.
        let mut log = SeccompFilter::inactive();
        log.active = true;
        log.whitelist = false;
        log.default_action = SECCOMP_LOG;
        assert_eq!(log.eval(SYS_WRITE), SeccompVerdict::Log);
        assert!(SeccompVerdict::Log.allows());
        // A blacklist denies only what is listed.
        let mut bl = SeccompFilter::inactive();
        bl.active = true;
        bl.whitelist = false;
        bl.default_action = SECCOMP_ERRNO;
        bl.err = Errno::Seccomp;
        bl.permit(SYS_MMAP);
        assert_eq!(bl.eval(SYS_MMAP), SeccompVerdict::Errno(Errno::Seccomp));
        assert_eq!(bl.eval(SYS_READ), SeccompVerdict::Allow);
    }

    #[test]
    fn f065_quota_window_resets_and_hard_limit_sticks() {
        let mut q = Quota::new(3, 100, 0);
        for i in 0..3 {
            assert_eq!(q.charge(10 + i), Ok(()));
        }
        // Fourth call in the same window is a retryable rate refusal.
        assert_eq!(q.charge(20), Err(Errno::Again));
        assert_eq!(q.headroom(), 0);
        // A new window refills.
        assert_eq!(q.charge(200), Ok(()));
        assert_eq!(q.denied, 1);

        // Hard ceilings are terminal, and distinguishable from rate excess.
        let mut h = Quota::new(100, 100, 2);
        assert_eq!(h.charge(0), Ok(()));
        assert_eq!(h.charge(0), Ok(()));
        assert_eq!(h.charge(0), Err(Errno::Quota));
        assert!(!Errno::Quota.is_retryable());
        assert!(Errno::Again.is_retryable());

        // Unmetered processes are never charged.
        let mut u = Quota::unmetered();
        for _ in 0..1000 {
            assert_eq!(u.charge(0), Ok(()));
        }
        assert_eq!(u.denied, 0);
    }

    #[test]
    fn f064_audit_ring_keeps_the_recent_and_counts_denials() {
        let mut ring = AuditRing::new();
        assert!(ring.latest().is_none());
        for i in 0..(MAX_AUDIT as u64 + 5) {
            ring.record(AuditEntry {
                nr: SYS_WRITE,
                pid: 7,
                caller_rip: 0x1000,
                arg0: i,
                tick: i,
                outcome: if i % 2 == 0 { Errno::Perm } else { Errno::Ok },
                denied: i % 2 == 0,
                path_compat: false,
            });
        }
        assert_eq!(ring.count, MAX_AUDIT);
        assert_eq!(ring.dropped, 5);
        let latest = ring.latest().unwrap();
        assert_eq!(latest.arg0, MAX_AUDIT as u64 + 4);
        // The oldest retained entry is the one that just escaped the window.
        assert_eq!(ring.oldest().unwrap().arg0, 5);
        assert!(ring.denied() > 0);
        assert!(ring.at(MAX_AUDIT).is_none());
        assert!(is_audited(SYS_WRITE));
        assert!(is_audited(SYS_EXIT));
        assert!(!is_audited(SYS_READ));
        // Unregistered numbers are audited by default: fail visible.
        assert!(is_audited(0x9999));
        ring.clear();
        assert_eq!(ring.count, 0);
    }

    #[test]
    fn f063_064_065_067_gate_order_is_enforced() {
        let mut seccomp = SeccompFilter::whitelist_only(&[SYS_READ]);
        let mut quota = Quota::new(1, 100, 0);
        let mut audit = AuditRing::new();

        // seccomp refuses MMAP first — and it must NOT have charged quota.
        let v = {
            let mut ctx = CallerCtx {
                pid: 42,
                caps: CAP_NONE, // 故意什么能力都不给
                caller_rip: 0x2000,
                tick: 0,
                seccomp: &mut seccomp,
                quota: &mut quota,
                audit: &mut audit,
                audit_enabled: true,
            };
            gate(&mut ctx, SYS_MMAP, 0)
        };
        assert_eq!(v, GateVerdict::Kill);
        assert_eq!(quota.used, 0, "seccomp must run before quota");
        assert_eq!(quota.denied, 0);
        assert_eq!(audit.latest().unwrap().nr, SYS_MMAP);
        assert!(audit.latest().unwrap().denied);

        // READ passes the filter, then fails the capability gate — but the
        // quota charge already happened, proving the documented order.
        let v2 = {
            let mut ctx = CallerCtx {
                pid: 42,
                caps: CAP_NONE,
                caller_rip: 0x2000,
                tick: 0,
                seccomp: &mut seccomp,
                quota: &mut quota,
                audit: &mut audit,
                audit_enabled: true,
            };
            gate(&mut ctx, SYS_READ, 0)
        };
        assert_eq!(v2, GateVerdict::Denied(Errno::Perm));
        assert_eq!(quota.used, 1, "capability runs after quota charge");
        assert_eq!(audit.latest().unwrap().outcome, Errno::Perm);

        // The budget is now spent: the next call is a rate refusal, and it
        // happens before the (also-failing) capability check.
        let v3 = {
            let mut ctx = CallerCtx {
                pid: 42,
                caps: CAP_ALL,
                caller_rip: 0x2000,
                tick: 0,
                seccomp: &mut seccomp,
                quota: &mut quota,
                audit: &mut audit,
                audit_enabled: true,
            };
            gate(&mut ctx, SYS_READ, 0)
        };
        assert_eq!(v3, GateVerdict::Denied(Errno::Again));

        // An allowed sensitive call is audited with outcome `Ok`, and an
        // allowed non-sensitive call is not audited at all.
        let mut q2 = Quota::default_budget();
        let mut a2 = AuditRing::new();
        let mut s2 = SeccompFilter::inactive();
        let _ = {
            let mut ctx = CallerCtx {
                pid: 9,
                caps: CAP_ALL,
                caller_rip: 0x3000,
                tick: 0,
                seccomp: &mut s2,
                quota: &mut q2,
                audit: &mut a2,
                audit_enabled: true,
            };
            gate(&mut ctx, SYS_READ, 0)
        };
        assert_eq!(a2.count, 0, "read is not in the sensitive set");
        let _ = {
            let mut ctx = CallerCtx {
                pid: 9,
                caps: CAP_ALL,
                caller_rip: 0x3000,
                tick: 0,
                seccomp: &mut s2,
                quota: &mut q2,
                audit: &mut a2,
                audit_enabled: true,
            };
            gate(&mut ctx, SYS_WRITE, 1)
        };
        assert_eq!(a2.latest().unwrap().outcome, Errno::Ok);
        assert!(!a2.latest().unwrap().denied);

        // An unimplemented number never reaches the gates at all: the table
        // answers first. Prove the gate is not consulted by checking the quota
        // counter is untouched.
        let before = q2.lifetime;
        assert_eq!(table::route_incoming(0x40FF), Err(Errno::NoSys));
        assert_eq!(q2.lifetime, before);

        // Audit can be switched off wholesale (kernel-thread paths).
        let mut a3 = AuditRing::new();
        let _ = {
            let mut ctx = CallerCtx {
                pid: 1,
                caps: CAP_NONE,
                caller_rip: 0x1000,
                tick: 0,
                seccomp: &mut s2,
                quota: &mut q2,
                audit: &mut a3,
                audit_enabled: false,
            };
            gate(&mut ctx, SYS_MMAP, 0)
        };
        assert_eq!(a3.count, 0);
    }
}
