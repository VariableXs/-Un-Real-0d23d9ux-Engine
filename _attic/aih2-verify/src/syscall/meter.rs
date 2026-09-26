//! AI-03 · F072 syscall fuzz / F073 调用仪表.
//!
//! F072 is the load-bearing test of this whole domain: a million rounds of
//! *deliberately hostile* input driven through the real validation code. The
//! contract is not "it returns the right error", it is **"it cannot panic"** —
//! a panic in a syscall validator is a kernel crash reachable from ring 3.
//! Every round therefore goes through the same functions the entry stub calls:
//! number routing (F053), pointer windows (F054), the gate chain (F063~F067)
//! and the mmap validator (F059). Nothing is special-cased for the fuzzer.
//!
//! F073 is the other half of the deal: you cannot tune what you cannot see. A
//! fixed-capacity meter keeps per-number call counts, cumulative time and a
//! ring of recent samples, from which p50/p99 are derived without allocating.

use super::calls::{self, MmapRequest, PAGE_SIZE, PROT_ALL};
use super::errno::Errno;
use super::guard::{self, AuditRing, CallerCtx, Quota, SeccompFilter};
use super::table::{self, BAND_NATIVE_LO};
use super::uaccess::{self, FlatMem, UserRegion};

// ---------------------------------------------------------------------------
// F073 — 仪表
// ---------------------------------------------------------------------------

/// Numbers tracked individually. Anything outside `SYS_NR_BASE..+MAX_SYS_NR`
/// is folded into [`SyscallMeter::out_of_band`] rather than dropped.
pub const MAX_SYS_NR: usize = 128;
/// Recent samples retained for percentile estimation.
pub const RING: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sample {
    pub nr: u32,
    pub ns: u32,
}

impl Sample {
    pub const fn zero() -> Sample {
        Sample { nr: 0, ns: 0 }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyStats {
    pub count: u32,
    pub min: u64,
    pub p50: u64,
    pub p99: u64,
    pub max: u64,
    pub mean: u64,
}

/// Fixed-capacity per-number ledger (F073).
pub struct SyscallMeter {
    counts: [u64; MAX_SYS_NR],
    total_ns: [u64; MAX_SYS_NR],
    ring: [Sample; RING],
    head: usize,
    ring_len: usize,
    pub observed: u64,
    pub out_of_band: u64,
    pub denied: u64,
}

impl SyscallMeter {
    pub const fn new() -> SyscallMeter {
        SyscallMeter {
            counts: [0u64; MAX_SYS_NR],
            total_ns: [0u64; MAX_SYS_NR],
            ring: [Sample::zero(); RING],
            head: 0,
            ring_len: 0,
            observed: 0,
            out_of_band: 0,
            denied: 0,
        }
    }

    fn slot(nr: u64) -> Option<usize> {
        if nr >= BAND_NATIVE_LO && nr < BAND_NATIVE_LO + MAX_SYS_NR as u64 {
            Some((nr - BAND_NATIVE_LO) as usize)
        } else {
            None
        }
    }

    pub fn observe(&mut self, nr: u64, ns: u64, denied: bool) {
        self.observed += 1;
        if denied {
            self.denied += 1;
        }
        match Self::slot(nr) {
            Some(i) => {
                self.counts[i] += 1;
                self.total_ns[i] += ns;
            }
            None => self.out_of_band += 1,
        }
        self.ring[self.head] = Sample {
            nr: nr as u32,
            ns: ns.min(u32::MAX as u64) as u32,
        };
        self.head = (self.head + 1) % RING;
        if self.ring_len < RING {
            self.ring_len += 1;
        }
    }

    pub fn count_of(&self, nr: u64) -> u64 {
        Self::slot(nr).map(|i| self.counts[i]).unwrap_or(0)
    }

    pub fn total_ns_of(&self, nr: u64) -> u64 {
        Self::slot(nr).map(|i| self.total_ns[i]).unwrap_or(0)
    }

    pub fn mean_ns_of(&self, nr: u64) -> u64 {
        let c = self.count_of(nr);
        if c == 0 {
            0
        } else {
            self.total_ns_of(nr) / c
        }
    }

    /// Hottest numbers, most-called first. Fixed output slice — no sort buffer
    /// beyond the caller's own array.
    pub fn top(&self, out: &mut [(u64, u64)]) -> usize {
        for s in out.iter_mut() {
            *s = (0, 0);
        }
        let mut n = 0;
        for (i, c) in self.counts.iter().enumerate() {
            if *c == 0 {
                continue;
            }
            let nr = BAND_NATIVE_LO + i as u64;
            // Insertion sort into the caller's slice: `out` is tiny.
            let mut pos = n.min(out.len());
            if pos >= out.len() {
                pos = out.len() - 1;
                if out[pos].1 >= *c {
                    continue;
                }
            } else {
                n += 1;
            }
            out[pos] = (nr, *c);
            while pos > 0 && out[pos - 1].1 < out[pos].1 {
                out.swap(pos - 1, pos);
                pos -= 1;
            }
        }
        n
    }

    /// Latency percentiles over the retained ring (F073). Sorts a copy so the
    /// ring order is preserved; `scratch` is caller-supplied to keep this
    /// allocation-free.
    pub fn latency(&self, scratch: &mut [u64; RING]) -> LatencyStats {
        let n = self.ring_len;
        if n == 0 {
            return LatencyStats { count: 0, min: 0, p50: 0, p99: 0, max: 0, mean: 0 };
        }
        for i in 0..n {
            scratch[i] = self.ring[i].ns as u64;
        }
        // Insertion sort: n <= 32, so anything fancier is noise.
        for i in 1..n {
            let v = scratch[i];
            let mut j = i;
            while j > 0 && scratch[j - 1] > v {
                scratch[j] = scratch[j - 1];
                j -= 1;
            }
            scratch[j] = v;
        }
        let mut sum = 0u64;
        for i in 0..n {
            sum += scratch[i];
        }
        LatencyStats {
            count: n as u32,
            min: scratch[0],
            p50: scratch[n / 2],
            p99: scratch[(n * 99 / 100).min(n - 1)],
            max: scratch[n - 1],
            mean: sum / n as u64,
        }
    }

    pub fn reset(&mut self) {
        self.counts = [0u64; MAX_SYS_NR];
        self.total_ns = [0u64; MAX_SYS_NR];
        self.ring = [Sample::zero(); RING];
        self.head = 0;
        self.ring_len = 0;
        self.observed = 0;
        self.out_of_band = 0;
        self.denied = 0;
    }
}

impl Default for SyscallMeter {
    fn default() -> Self {
        Self::new()
    }
}

/// Splitmix64 — the fuzzer's only source of randomness, so a failing round is
/// reproducible from its seed alone.
pub const fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ---------------------------------------------------------------------------
// F072 — fuzz
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FuzzReport {
    pub rounds: u32,
    /// Rounds that produced a successful result.
    pub accepted: u32,
    /// Rounds correctly refused (any `Errno`).
    pub rejected: u32,
    /// Rounds the number table itself refused with `ENOSYS`.
    pub enosys: u32,
    /// Rounds that needed the compat translation.
    pub compat: u32,
}

impl FuzzReport {
    pub const fn total(&self) -> u32 {
        self.accepted + self.rejected
    }
}

/// F072a — hostile *numbers* through routing, the gate chain and the handlers.
pub fn fuzz_dispatch(rounds: u32, seed: u64) -> FuzzReport {
    let mut r = FuzzReport::default();
    let mut s = seed;
    let mut seccomp = SeccompFilter::inactive();
    let mut quota = Quota::default_budget();
    let mut audit = AuditRing::new();
    let mut fds = calls::FdTable::with_stdio();
    let mut pipes = calls::PipeTable::new();
    let mut console = calls::ConsoleSink::new();
    let mut mem = FlatMem::new(0x0000_0000_0001_0000);
    let region = UserRegion::standard();

    for _ in 0..rounds {
        // Three quarters of the rounds must land *inside* a real band: a pure
        // `u64` draws would spend every round in ENOSYS and never exercise the
        // handlers, the compat translation or the mmap validator.
        let pick = splitmix64(&mut s);
        let raw_nr = match pick % 4 {
            0 => splitmix64(&mut s),
            1 => table::SYS_NR_BASE + (splitmix64(&mut s) % 40),
            2 => table::BAND_COMPAT_LO + (splitmix64(&mut s) % 40),
            _ => splitmix64(&mut s) % 0x8000,
        };
        let caps = splitmix64(&mut s);
        r.rounds += 1;

        // Every hostile number must land on a defined decision.
        let nr = match table::route_incoming(raw_nr) {
            Ok(route) => {
                if route.linux_nr != route.varix_nr {
                    r.compat += 1;
                }
                route.varix_nr
            }
            Err(_) => {
                r.enosys += 1;
                continue;
            }
        };

        let verdict = {
            let mut ctx = CallerCtx {
                pid: 1,
                caps,
                caller_rip: 0x1000,
                tick: r.rounds as u64,
                seccomp: &mut seccomp,
                quota: &mut quota,
                audit: &mut audit,
                audit_enabled: true,
            };
            guard::gate(&mut ctx, nr, splitmix64(&mut s))
        };
        match verdict {
            guard::GateVerdict::Denied(_) | guard::GateVerdict::Kill => {
                r.rejected += 1;
                continue;
            }
            guard::GateVerdict::Proceed => {}
        }

        // Args are pure garbage from here on. Every arm must return, never trap.
        let a0 = splitmix64(&mut s);
        let a1 = splitmix64(&mut s);
        let a2 = splitmix64(&mut s);
        let outcome: Result<u64, Errno> = if nr == table::SYS_READ {
            let mut buf = [0u8; 32];
            calls::sys_read(&mut fds, &mut pipes, a0 as u32, &mut buf).map(|n| n as u64)
        } else if nr == table::SYS_WRITE {
            let blob = [0u8; 16];
            calls::sys_write(&mut fds, &mut pipes, &mut console, a0 as u32, &blob).map(|n| n as u64)
        } else if nr == table::SYS_MMAP {
            let req = MmapRequest {
                addr: a0,
                len: a1,
                prot: (a2 & 0xFFFF) as u32,
                flags: ((a2 >> 16) & 0xFFFF) as u32,
                fd: (a0 & 0xFF) as i32 - 128,
                offset: a1,
            };
            calls::validate_mmap(&req, a0, 64, 64).map(|p| p.addr)
        } else if nr == table::SYS_CLOCK {
            let mut c = calls::ClockSource::new(1);
            c.advance(1, 1);
            calls::clock_gettime(&c, calls::ClockId::Monotonic).map(|(sec, _)| sec)
        } else if nr == table::SYS_DUP {
            fds.dup_from(a0 as u32, (a1 & 7) as u32).map(|fd| fd as u64)
        } else if nr == table::SYS_PIPE {
            pipes.pipe(&mut fds).map(|(rd, _)| rd as u64)
        } else {
            // Numbers with no fuzz arm still must survive a pointer sweep.
            let mut buf = [0u8; 8];
            uaccess::copy_from_user(&mem, &region, &mut buf, a0).map(|n| n as u64)
        };
        let _ = uaccess::copy_to_user(&mut mem, &region, a1, b"fuzz");

        match outcome {
            Ok(_) => r.accepted += 1,
            Err(_) => r.rejected += 1,
        }
    }
    r
}

/// F072b — hostile *pointers* through the parameter-safety layer.
pub fn fuzz_pointers(rounds: u32, seed: u64) -> FuzzReport {
    let mut r = FuzzReport::default();
    let mut s = seed;
    let mem = FlatMem::new(0x0000_0000_0001_0000);
    let region = UserRegion::standard();
    let mut scratch = [0u8; 64];
    for _ in 0..rounds {
        r.rounds += 1;
        let ptr = splitmix64(&mut s);
        let len = splitmix64(&mut s);
        let n = (len % 64) as usize;
        let res = uaccess::copy_from_user_partial(&mem, &region, &mut scratch[..n], ptr);
        let _ = uaccess::copy_cstr_from_user(&mem, &region, &mut scratch, ptr);
        let _ = uaccess::read_user_u64(&mem, &region, ptr);
        match res {
            Ok(_) => r.accepted += 1,
            Err(p) => {
                // A partial failure must always carry a real error and a byte
                // count that cannot exceed the buffer.
                assert!(p.moved <= scratch.len());
                assert_ne!(p.err, Errno::Ok);
                r.rejected += 1;
            }
        }
    }
    r
}

/// F072c — hostile *protection/flag* combinations through the mmap validator.
pub fn fuzz_mmap(rounds: u32, seed: u64) -> FuzzReport {
    let mut r = FuzzReport::default();
    let mut s = seed;
    for _ in 0..rounds {
        r.rounds += 1;
        let req = MmapRequest {
            addr: splitmix64(&mut s),
            len: splitmix64(&mut s),
            prot: (splitmix64(&mut s) & 0xFFFF) as u32,
            flags: (splitmix64(&mut s) & 0xFFFF) as u32,
            fd: (splitmix64(&mut s) & 0xFF) as i32 - 128,
            offset: splitmix64(&mut s),
        };
        // W^X must hold for every accepted request, without exception.
        match calls::validate_mmap(&req, 0x1000, 64, 64) {
            Ok(p) => {
                assert!(calls::prot_wx_ok(req.prot), "W^X accepted W+X: {req:?}");
                assert!(p.pages >= 1);
                assert_eq!(p.addr % PAGE_SIZE, 0);
                assert!(req.prot & !PROT_ALL == 0);
                r.accepted += 1;
            }
            Err(e) => {
                assert!(!matches!(e, Errno::Ok));
                r.rejected += 1;
            }
        }
    }
    r
}

/// F072 — all three sweeps, the entry point the self-test uses.
pub fn fuzz_run(rounds: u32, seed: u64) -> FuzzReport {
    let a = fuzz_dispatch(rounds, seed);
    let b = fuzz_pointers(rounds, seed ^ 0xA5A5_A5A5);
    let c = fuzz_mmap(rounds, seed ^ 0x5A5A_5A5A);
    FuzzReport {
        rounds: a.rounds + b.rounds + c.rounds,
        accepted: a.accepted + b.accepted + c.accepted,
        rejected: a.rejected + b.rejected + c.rejected,
        enosys: a.enosys,
        compat: a.compat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syscall::table::{SYS_CLOCK, SYS_MMAP, SYS_READ, SYS_WRITE};

    #[test]
    fn f073_meter_tracks_counts_and_percentiles() {
        let mut m = SyscallMeter::new();
        m.observe(SYS_CLOCK, 100, false);
        m.observe(SYS_CLOCK, 200, false);
        m.observe(SYS_CLOCK, 300, false);
        m.observe(SYS_WRITE, 900, true);
        m.observe(0xDEAD_BEEF, 10, false);

        assert_eq!(m.observed, 5);
        assert_eq!(m.denied, 1);
        assert_eq!(m.out_of_band, 1, "unknown numbers are counted, not dropped");
        assert_eq!(m.count_of(SYS_CLOCK), 3);
        assert_eq!(m.total_ns_of(SYS_CLOCK), 600);
        assert_eq!(m.mean_ns_of(SYS_CLOCK), 200);
        assert_eq!(m.mean_ns_of(SYS_MMAP), 0);

        let mut scratch = [0u64; RING];
        let st = m.latency(&mut scratch);
        // Samples: 100, 200, 300, 900, 10 → sorted 10,100,200,300,900.
        assert_eq!(st.count, 5);
        assert_eq!(st.min, 10);
        assert_eq!(st.max, 900);
        assert_eq!(st.p50, 200);
        assert_eq!(st.p99, 900);
        assert_eq!(st.mean, 302);

        let mut top = [(0u64, 0u64); 4];
        let n = m.top(&mut top);
        assert!(n >= 2);
        assert_eq!(top[0], (SYS_CLOCK, 3), "hottest call first");

        // An empty meter answers zeros rather than dividing by zero.
        let empty = SyscallMeter::new();
        let st0 = empty.latency(&mut scratch);
        assert_eq!(st0.count, 0);
        assert_eq!(st0.p50, 0);
        assert_eq!(empty.mean_ns_of(SYS_READ), 0);

        m.reset();
        assert_eq!(m.observed, 0);
        assert_eq!(m.count_of(SYS_CLOCK), 0);
    }

    #[test]
    fn f072_fuzz_is_deterministic_and_panic_free() {
        // 1e6 rounds total across the three sweeps: the amount the requirement
        // names. Deterministic, so a failure is reproducible from the seed.
        let a = fuzz_run(334_000, 0x1234_5678_9ABC_DEF0);
        let b = fuzz_run(334_000, 0x1234_5678_9ABC_DEF0);
        assert_eq!(a, b, "same seed must give the same report");
        assert_eq!(a.rounds, 3 * 334_000);
        assert!(a.accepted > 0, "some inputs must be legal");
        assert!(a.rejected > 0, "some inputs must be refused");
        assert!(a.enosys > 0, "wild numbers must hit ENOSYS");
        assert!(a.compat > 0, "legacy band must exercise the compat layer");
        assert_eq!(a.total(), a.accepted + a.rejected);

        // A different seed explores a different corner without crashing.
        let c = fuzz_run(1_000, 0xFEED_FACE_CAFE_BEEF);
        assert_eq!(c.rounds, 3000);
    }

    #[test]
    fn f072_extremes_do_not_panic() {
        // The specific shapes that historically break range maths.
        assert_eq!(calls::page_align_up(u64::MAX), u64::MAX & !(PAGE_SIZE - 1));
        let mut scratch = [0u8; 64];
        let mem = FlatMem::new(0x1000_0000);
        let region = UserRegion::standard();
        for ptr in [0u64, 1, u64::MAX, u64::MAX - 1, 0x7FFF_FFFF_FFFF, 0x8000_0000_0000] {
            assert!(uaccess::copy_from_user_partial(&mem, &region, &mut scratch, ptr).is_err());
        }
        for len in [0u64, 1, u64::MAX, u64::MAX / 2] {
            let req = MmapRequest {
                addr: 0,
                len,
                prot: calls::PROT_READ | calls::PROT_WRITE,
                flags: calls::MAP_PRIVATE | calls::MAP_ANONYMOUS,
                fd: -1,
                offset: 0,
            };
            let _ = calls::validate_mmap(&req, 0, u64::MAX, 64);
        }
        // Zero-length anonymous mappings are always refused, never placed.
        let zero = MmapRequest { addr: 0, len: 0, prot: 0, flags: calls::MAP_ANONYMOUS | calls::MAP_PRIVATE, fd: -1, offset: 0 };
        assert_eq!(calls::validate_mmap(&zero, 0, 64, 64), Err(Errno::Inval));
    }
}
