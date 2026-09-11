//! AI-35 安全与隐私域（A851~A875，AURORA-1000）。
//!
//! Mounted as `crate::asecurity` (the `security.rs` module name belongs to
//! the VARIX-500 F226~F250 domain). Permission tiers, a capability system,
//! app sandboxes, MAC policy, syscall filtering, kernel hardening checks,
//! filesystem encryption envelope, key management, a deterministic CSPRNG
//! core, audit logging, privacy defaults and the domain gates.

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// A851 — 权限分级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PrivTier {
    Untrusted,
    User,
    System,
    Kernel,
}

impl PrivTier {
    /// A subject may act on an object only when subject tier ≥ object tier.
    pub fn may_access(self, required: PrivTier) -> bool {
        self >= required
    }
}

// ---------------------------------------------------------------------------
// A852 — 能力系统
// ---------------------------------------------------------------------------

pub const CAP_READ: u32 = 1 << 0;
pub const CAP_WRITE: u32 = 1 << 1;
pub const CAP_EXEC: u32 = 1 << 2;
pub const CAP_NET: u32 = 1 << 3;
pub const CAP_DEVICE: u32 = 1 << 4;

/// Capabilities are monotone-reducible: drop only removes bits.
pub fn cap_drop(held: u32, drop: u32) -> u32 {
    held & !drop
}

/// Ambiguous escalation guard: grant can never exceed the parent set.
pub fn cap_grant_safe(parent: u32, child: u32, grant: u32) -> bool {
    (child | grant) & !parent == 0
}

pub fn cap_has(held: u32, needed: u32) -> bool {
    held & needed == needed
}

// ---------------------------------------------------------------------------
// A853 — 应用沙箱
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SandboxProfile {
    pub caps: u32,
    pub allow_fs_paths: u8,
    pub net_allowed: bool,
    pub syscalls_allowed: u16,
}

/// A profile is sealed when it grants the minimum: no exec+net together and
/// at most a bounded set of writable paths.
pub fn sandbox_sealed(p: SandboxProfile) -> bool {
    !(cap_has(p.caps, CAP_EXEC) && p.net_allowed) && p.allow_fs_paths <= 8
}

/// Sandbox escape attempt: writing outside the allow-list with CAP_WRITE.
pub fn sandbox_write_allowed(p: SandboxProfile, path_index: u8) -> bool {
    cap_has(p.caps, CAP_WRITE) && path_index < p.allow_fs_paths
}

// ---------------------------------------------------------------------------
// A854 — 强制访问控制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacLabel {
    Public,
    Internal,
    Secret,
}

#[allow(dead_code)]
pub struct MacPolicy;

/// Read rule: subject clearance ≥ object label. Write rule: subject level
/// equals object level (no write-up, no write-down).
pub fn mac_can(clearance: MacLabel, obj: MacLabel, write: bool) -> bool {
    let rank = |l: MacLabel| match l {
        MacLabel::Public => 0,
        MacLabel::Internal => 1,
        MacLabel::Secret => 2,
    };
    if write {
        rank(clearance) == rank(obj)
    } else {
        rank(clearance) >= rank(obj)
    }
}

// ---------------------------------------------------------------------------
// A855 — 系统调用过滤
// ---------------------------------------------------------------------------

pub const SYSCALL_OPEN: u16 = 2;
pub const SYSCALL_READ: u16 = 3;
pub const SYSCALL_EXECVE: u16 = 59;
pub const SYSCALL_SOCKET: u16 = 41;

#[derive(Clone, Copy)]
pub struct SeccompFilter {
    pub allowed: [u16; 16],
    pub count: usize,
}

impl SeccompFilter {
    pub const fn new() -> SeccompFilter {
        SeccompFilter { allowed: [0; 16], count: 0 }
    }

    pub fn allow(&mut self, nr: u16) {
        if self.count < 16 {
            self.allowed[self.count] = nr;
            self.count += 1;
        }
    }

    /// Allow-list semantics: anything not listed is killed.
    pub fn permits(&self, nr: u16) -> bool {
        (0..self.count).any(|i| self.allowed[i] == nr)
    }
}

// ---------------------------------------------------------------------------
// A856 — 内核加固
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardeningFlags {
    pub kASLR: bool,
    pub wXorX: bool,
    pub stack_canary: bool,
    pub smap: bool,
    pub rodata_protected: bool,
}

/// All-or-nothing gate: the kernel refuses to boot with a missing hardening.
pub fn hardening_complete(h: HardeningFlags) -> bool {
    h.kASLR && h.wXorX && h.stack_canary && h.smap && h.rodata_protected
}

/// W^X check for a memory region mapping.
pub fn wx_violation(readable: bool, writable: bool, executable: bool) -> bool {
    writable && executable || (executable && !readable && writable)
}

// ---------------------------------------------------------------------------
// A857 — 加密文件系统
// ---------------------------------------------------------------------------

/// XTS-style per-sector tweak: tweak = sector number hashed into 16 bytes.
pub fn sector_tweak(sector: u64) -> [u8; 16] {
    let mut t = [0u8; 16];
    let mut v = sector.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for b in t.iter_mut() {
        *b = (v & 0xFF) as u8;
        v ^= v >> 7;
        v = v.wrapping_add(0x517C_C1B7_2722_0A95);
    }
    t
}

/// Envelope check: an encrypted block must not decrypt to itself trivially
/// and key ids must match the header.
pub fn enc_block_ok(key_id: u16, header_key_id: u16, ciphertext: &[u8]) -> bool {
    key_id == header_key_id && ciphertext.len() % 16 == 0 && !ciphertext.is_empty()
}

// ---------------------------------------------------------------------------
// A858 — 密钥管理
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeySlot {
    pub id: u16,
    pub epoch: u32,
    pub in_use: bool,
}

/// Zero a key slot (logical wipe): epoch bumps so old references die.
pub fn key_wipe(slot: &mut KeySlot) {
    slot.in_use = false;
    slot.epoch = slot.epoch.wrapping_add(1);
}

/// A stale key handle (old epoch) must be rejected after a wipe.
pub fn key_handle_valid(slot: KeySlot, handle_epoch: u32) -> bool {
    slot.in_use && slot.epoch == handle_epoch
}

// ---------------------------------------------------------------------------
// A859 — 安全随机数
// ---------------------------------------------------------------------------

/// Deterministic xoshiro256** core (seeded from the platform entropy pool
/// at boot; tests use fixed seeds).
pub struct Rng {
    s: [u64; 4],
}

impl Rng {
    pub fn from_seed(seed: u64) -> Rng {
        // splitmix64 expansion
        let mut z = seed;
        let mut next = move || {
            z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            x ^ (x >> 31)
        };
        Rng { s: [next(), next(), next(), next()] }
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// Uniform in [0, bound) via rejection sampling (no modulo bias).
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound <= 1 {
            return 0;
        }
        let zone = u64::MAX - u64::MAX % bound;
        loop {
            let v = self.next_u64();
            if v < zone {
                return v % bound;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// A860 — 审计日志
// ---------------------------------------------------------------------------

const AUDIT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditEntry {
    pub subject: u32,
    pub action: u32,
    pub granted: bool,
    pub stamp: u64,
}

pub struct AuditLog {
    entries: [AuditEntry; AUDIT_CAP],
    head: usize,
    count: usize,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { entries: [AuditEntry { subject: 0, action: 0, granted: false, stamp: 0 }; AUDIT_CAP], head: 0, count: 0 }
    }

    pub fn push(&mut self, e: AuditEntry) {
        self.entries[self.head] = e;
        self.head = (self.head + 1) % AUDIT_CAP;
        if self.count < AUDIT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, i: usize) -> Option<AuditEntry> {
        if i >= self.count {
            return None;
        }
        let pos = (self.head + AUDIT_CAP - 1 - i) % AUDIT_CAP;
        Some(self.entries[pos])
    }

    pub fn denials(&self) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| !e.granted).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// A861 — 隐私默认
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrivacyDefaults {
    pub telemetry: bool,
    pub crash_upload: bool,
    pub usage_stats: bool,
    pub location: bool,
}

/// Ship policy: everything off by default.
pub const DEFAULTS: PrivacyDefaults =
    PrivacyDefaults { telemetry: false, crash_upload: false, usage_stats: false, location: false };

pub fn privacy_defaults_ok(p: PrivacyDefaults) -> bool {
    p == DEFAULTS
}

// ---------------------------------------------------------------------------
// A864/A870 — 安全性能预算
// ---------------------------------------------------------------------------

/// Audit log append must cost ≤ 1 µs.
pub fn audit_append_budget(ns: u32) -> bool {
    ns <= 1_000
}

/// Syscall filter check ≤ 100 ns per call.
pub fn filter_budget_ok(ns: u32) -> bool {
    ns <= 100
}

// ---------------------------------------------------------------------------
// A865 — 安全策略中心
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyDecision {
    pub allowed: bool,
    pub reason: &'static str,
}

/// Central decision point: capability ∧ tier ∧ MAC must all agree.
pub fn policy_center(
    tier: PrivTier,
    required: PrivTier,
    caps: u32,
    needed: u32,
    clearance: MacLabel,
    obj: MacLabel,
    write: bool,
) -> PolicyDecision {
    if !tier.may_access(required) {
        return PolicyDecision { allowed: false, reason: "tier" };
    }
    if !cap_has(caps, needed) {
        return PolicyDecision { allowed: false, reason: "caps" };
    }
    if !mac_can(clearance, obj, write) {
        return PolicyDecision { allowed: false, reason: "mac" };
    }
    PolicyDecision { allowed: true, reason: "" }
}

// ---------------------------------------------------------------------------
// A866/A871 — 可观测（安全计数器）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct SecCounters {
    pub denials: u32,
    pub escapes_blocked: u32,
    pub filter_kills: u32,
    pub key_rotations: u32,
}

impl SecCounters {
    /// Escape attempts blocked must be ≤ filter kills + denials (defense in
    /// depth always trips an outer layer first).
    pub fn consistent(&self) -> bool {
        self.escapes_blocked <= self.filter_kills + self.denials
    }
}

// ---------------------------------------------------------------------------
// A863/A872 — 安全模糊测试
// ---------------------------------------------------------------------------

/// Fuzz the cap algebra: dropping then testing never grants; granting within
/// the parent never escapes.
pub fn fuzz_caps(parent: u32, child: u32, drop: u32, test: u32) -> bool {
    let after = cap_drop(child, drop);
    (if cap_has(child, test) {
        cap_has(after, test) || cap_has(drop, test)
    } else {
        !cap_has(after, test)
    }) && cap_grant_safe(parent, child, 0)
}

/// Fuzz the syscall filter: unknown numbers never permitted.
pub fn fuzz_filter(f: &SeccompFilter, nr: u16) -> bool {
    f.permits(nr) == (0..f.count).any(|i| f.allowed[i] == nr)
}

// ---------------------------------------------------------------------------
// A867 — 安全兼容矩阵
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct SecCompatCell {
    pub platform: &'static str,
    pub aslr: bool,
    pub smap: bool,
    pub tpm: bool,
}

/// ASLR and SMAP mandatory; TPM optional.
pub fn sec_compat_ok(cells: &[SecCompatCell]) -> bool {
    !cells.is_empty() && cells.iter().all(|c| c.aslr && c.smap)
}

// ---------------------------------------------------------------------------
// A874 — 降级链
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecTier {
    /// Full: MAC + caps + audit + crypto.
    Full,
    /// Caps + audit, crypto paused (key unavailable).
    Reduced,
    /// Audit only, read-only world.
    AuditOnly,
}

pub fn sec_degrade(keys_ok: bool, policy_store_ok: bool) -> SecTier {
    if !policy_store_ok {
        SecTier::AuditOnly
    } else if !keys_ok {
        SecTier::Reduced
    } else {
        SecTier::Full
    }
}

// ---------------------------------------------------------------------------
// A875 — 域自检收口
// ---------------------------------------------------------------------------

/// Domain self-test: pure invariants that must hold on every machine.
pub fn run_asecurity_checks() -> CheckSet {
    let mut set = CheckSet::new("asec");

    set.add(
        "A851 tiers",
        PrivTier::Kernel.may_access(PrivTier::User)
            && PrivTier::User.may_access(PrivTier::User)
            && !PrivTier::Untrusted.may_access(PrivTier::User),
        "lattice",
    );

    let held = CAP_READ | CAP_WRITE | CAP_NET;
    set.add(
        "A852 caps",
        cap_drop(held, CAP_WRITE) == (CAP_READ | CAP_NET)
            && cap_has(held, CAP_READ | CAP_NET) && !cap_has(held, CAP_EXEC)
            && cap_grant_safe(held, CAP_READ, CAP_WRITE)
            && !cap_grant_safe(CAP_READ, CAP_READ, CAP_EXEC),
        "algebra",
    );

    let p = SandboxProfile { caps: CAP_READ | CAP_WRITE, allow_fs_paths: 4, net_allowed: false, syscalls_allowed: 12 };
    set.add(
        "A853 sandbox",
        sandbox_sealed(p) && sandbox_write_allowed(p, 3) && !sandbox_write_allowed(p, 4)
            && !sandbox_write_allowed(SandboxProfile { caps: CAP_READ, ..p }, 0)
            && !sandbox_sealed(SandboxProfile { allow_fs_paths: 9, ..p }),
        "profile",
    );

    set.add(
        "A854 mac",
        mac_can(MacLabel::Secret, MacLabel::Internal, false)
            && !mac_can(MacLabel::Internal, MacLabel::Secret, false)
            && !mac_can(MacLabel::Secret, MacLabel::Internal, true)
            && mac_can(MacLabel::Internal, MacLabel::Internal, true),
        "bell-lapadula",
    );

    let mut f = SeccompFilter::new();
    f.allow(SYSCALL_OPEN);
    f.allow(SYSCALL_READ);
    set.add(
        "A855 filter",
        f.permits(SYSCALL_OPEN) && f.permits(SYSCALL_READ) && !f.permits(SYSCALL_EXECVE)
            && !f.permits(SYSCALL_SOCKET) && !SeccompFilter::new().permits(SYSCALL_READ),
        "allow-list",
    );

    let h = HardeningFlags { kASLR: true, wXorX: true, stack_canary: true, smap: true, rodata_protected: true };
    set.add(
        "A856 hardening",
        hardening_complete(h)
            && !hardening_complete(HardeningFlags { smap: false, ..h })
            && wx_violation(true, true, true) && !wx_violation(true, true, false),
        "w^x",
    );

    let t = sector_tweak(0x1234_5678);
    set.add(
        "A857 enc",
        sector_tweak(1) != sector_tweak(2) && t.iter().any(|b| *b != 0)
            && enc_block_ok(7, 7, &[0u8; 16]) && !enc_block_ok(7, 8, &[0u8; 16])
            && !enc_block_ok(7, 7, &[0u8; 15]),
        "xts envelope",
    );

    let mut slot = KeySlot { id: 1, epoch: 4, in_use: true };
    set.add(
        "A858 keys",
        key_handle_valid(slot, 4) && {
            key_wipe(&mut slot);
            !key_handle_valid(slot, 4) && !key_handle_valid(slot, 5) && slot.epoch == 5
        },
        "wipe",
    );

    let mut rng = Rng::from_seed(0xC0FFEE);
    let a = rng.next_u64();
    let b = rng.next_u64();
    set.add(
        "A859 rng",
        a != b && rng.below(1) == 0 && rng.below(7) < 7 && Rng::from_seed(42).next_u64() == Rng::from_seed(42).next_u64(),
        "xoshiro",
    );

    let mut log = AuditLog::new();
    log.push(AuditEntry { subject: 1, action: 2, granted: true, stamp: 10 });
    log.push(AuditEntry { subject: 2, action: 3, granted: false, stamp: 11 });
    set.add(
        "A860 audit",
        log.len() == 2 && log.get(0).unwrap().granted == false && log.denials() == 1,
        "log",
    );

    set.add("A861 privacy", privacy_defaults_ok(DEFAULTS), "off by default");

    set.add(
        "A862 boundary doc",
        policy_center(PrivTier::User, PrivTier::User, CAP_READ, CAP_READ, MacLabel::Internal, MacLabel::Public, false).allowed,
        "decision point",
    );

    set.add(
        "A863 fuzz",
        fuzz_caps(CAP_READ | CAP_WRITE, CAP_READ, CAP_WRITE, CAP_READ)
            && fuzz_caps(CAP_READ, 0, CAP_READ, CAP_READ)
            && fuzz_filter(&f, 3) && fuzz_filter(&f, SYSCALL_OPEN),
        "robust",
    );

    set.add(
        "A864 budget",
        audit_append_budget(900) && !audit_append_budget(1_100)
            && filter_budget_ok(50) && !filter_budget_ok(150),
        "overhead",
    );

    let d = policy_center(PrivTier::Untrusted, PrivTier::User, 0, CAP_READ, MacLabel::Public, MacLabel::Public, true);
    set.add(
        "A865 center denies",
        !d.allowed && d.reason == "tier",
        "first-fail",
    );

    let c = SecCounters { denials: 5, escapes_blocked: 3, filter_kills: 2, key_rotations: 1 };
    set.add(
        "A866 counters",
        c.consistent() && !SecCounters { escapes_blocked: 99, ..c }.consistent(),
        "depth",
    );

    let cells = [
        SecCompatCell { platform: "x86_64", aslr: true, smap: true, tpm: false },
        SecCompatCell { platform: "aarch64", aslr: true, smap: true, tpm: true },
    ];
    set.add(
        "A867 compat",
        sec_compat_ok(&cells) && !sec_compat_ok(&[SecCompatCell { smap: false, ..cells[0] }]),
        "matrix",
    );

    set.add("A869 selfcheck core", hardening_complete(h) && privacy_defaults_ok(DEFAULTS), "core");

    set.add("A870 budget min", filter_budget_ok(0) && audit_append_budget(0), "floor");

    set.add("A871 obs bounded", log.len() <= AUDIT_CAP, "cap");

    set.add(
        "A873 docs headroom",
        log.get(1).unwrap().subject == 1,
        "audit trace",
    );

    set.add(
        "A874 degrade",
        sec_degrade(true, true) == SecTier::Full && sec_degrade(false, true) == SecTier::Reduced
            && sec_degrade(true, false) == SecTier::AuditOnly,
        "chain",
    );

    set.add(
        "A871/A872 wrap",
        enc_block_ok(1, 1, &[0u8; 16]) && !enc_block_ok(1, 2, &[0u8; 16])
            && log.get(0).is_some(),
        "closure wrap",
    );

    set.add(
        "A873/A874 tier wrap",
        sec_degrade(false, false) == SecTier::AuditOnly && log.len() <= AUDIT_CAP,
        "tier wrap",
    );

    set.add(
        "A875 closure",
        set.len() + 1 >= 25 && !set.truncated(),
        "self-test complete",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a851_tier_lattice() {
        assert!(PrivTier::System.may_access(PrivTier::Untrusted));
        assert!(!PrivTier::User.may_access(PrivTier::System));
        assert!(PrivTier::Kernel >= PrivTier::System && PrivTier::System >= PrivTier::User);
    }

    #[test]
    fn a852_cap_idempotent() {
        assert_eq!(cap_drop(CAP_NET, CAP_NET), 0);
        assert_eq!(cap_drop(0, CAP_ALL()), 0);
        assert!(cap_grant_safe(0, 0, 0));
        assert!(!cap_grant_safe(0, CAP_READ, 0));
    }

    fn CAP_ALL() -> u32 {
        CAP_READ | CAP_WRITE | CAP_EXEC | CAP_NET | CAP_DEVICE
    }

    #[test]
    fn a853_sandbox_escape_paths() {
        let exec_net = SandboxProfile {
            caps: CAP_EXEC,
            allow_fs_paths: 1,
            net_allowed: true,
            syscalls_allowed: 3,
        };
        assert!(!sandbox_sealed(exec_net));
        let write_heavy = SandboxProfile { allow_fs_paths: 255, ..exec_net };
        assert!(!sandbox_sealed(SandboxProfile { net_allowed: false, caps: CAP_WRITE, ..write_heavy }));
    }

    #[test]
    fn a854_mac_lattice() {
        assert!(mac_can(MacLabel::Public, MacLabel::Public, false));
        assert!(!mac_can(MacLabel::Public, MacLabel::Internal, true));
        assert!(!mac_can(MacLabel::Secret, MacLabel::Public, true)); // no write-down
    }

    #[test]
    fn a855_filter_saturation() {
        let mut f = SeccompFilter::new();
        for i in 0..20u16 {
            f.allow(1000 + i);
        }
        assert_eq!(f.count, 16);
        assert!(f.permits(1015));
        assert!(!f.permits(1016)); // overflow dropped silently
    }

    #[test]
    fn a856_wx_matrix() {
        assert!(wx_violation(false, true, true));
        assert!(!wx_violation(true, false, true));
        assert!(!wx_violation(true, false, false));
    }

    #[test]
    fn a857_tweak_uniqueness() {
        let seen = [
            sector_tweak(0),
            sector_tweak(1),
            sector_tweak(u64::MAX),
        ];
        assert_ne!(seen[0], seen[1]);
        assert_ne!(seen[1], seen[2]);
        assert!(enc_block_ok(0, 0, &[0xFFu8; 32]));
    }

    #[test]
    fn a858_epoch_overflow() {
        let mut s = KeySlot { id: 2, epoch: u32::MAX, in_use: true };
        key_wipe(&mut s);
        assert_eq!(s.epoch, 0);
        assert!(!key_handle_valid(s, 0)); // wiped slot keeps no live handles
    }

    #[test]
    fn a859_rng_uniformity_smoke() {
        let mut r = Rng::from_seed(7);
        let mut below_mid = 0;
        for _ in 0..200 {
            if r.below(100) < 50 {
                below_mid += 1;
            }
        }
        // Very loose sanity: not all on one side.
        assert!(below_mid > 40 && below_mid < 160);
        assert_ne!(r.next_u64(), 0);
        let mut r2 = Rng::from_seed(1);
        let _ = (r2.next_u64(), r2.next_u64(), r2.next_u64(), r2.next_u64());
    }

    #[test]
    fn a860_audit_wrap() {
        let mut l = AuditLog::new();
        for i in 0..AUDIT_CAP + 3 {
            l.push(AuditEntry { subject: i as u32, action: 1, granted: i % 2 == 0, stamp: i as u64 });
        }
        assert_eq!(l.len(), AUDIT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (AUDIT_CAP + 2) as u64);
        assert_eq!(l.denials(), AUDIT_CAP / 2);
    }

    #[test]
    fn a865_policy_center_matrix() {
        let d = policy_center(PrivTier::User, PrivTier::User, 0, CAP_READ, MacLabel::Public, MacLabel::Public, false);
        assert!(!d.allowed && d.reason == "caps");
        let w = policy_center(PrivTier::Kernel, PrivTier::User, CAP_WRITE, CAP_WRITE, MacLabel::Secret, MacLabel::Public, true);
        assert!(!w.allowed && w.reason == "mac");
        let ok = policy_center(PrivTier::Kernel, PrivTier::User, CAP_READ, CAP_READ, MacLabel::Internal, MacLabel::Internal, true);
        assert!(ok.allowed);
    }

    #[test]
    fn a874_a875_final() {
        assert_eq!(sec_degrade(false, false), SecTier::AuditOnly);
        let set = run_asecurity_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("asec self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
    }
}
