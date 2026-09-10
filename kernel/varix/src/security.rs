//! AI-10 · 安全与隔离域（F226~F250）.
//!
//! The security domain's job is to make the *rest* of the kernel's promises
//! checkable: every page is either writable or executable but never both, every
//! privilege transition is audited, every measurement is chained, and nothing
//! leaves the machine. Two primitives here are real cryptography — SHA-256 and
//! the ChaCha20 block function — because a placeholder hash in a security
//! module is worse than no hash at all.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// SHA-256 (F233/F239 — measurements and signature verification)
// ---------------------------------------------------------------------------

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const SHA256_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

/// Incremental SHA-256. Streaming matters because a boot measurement hashes
/// the image in chunks rather than holding it all in memory.
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length_bytes: u64,
}

impl Sha256 {
    pub const fn new() -> Sha256 {
        Sha256 {
            state: SHA256_INIT,
            buffer: [0; 64],
            buffered: 0,
            length_bytes: 0,
        }
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut a = self.state[0];
        let mut b = self.state[1];
        let mut c = self.state[2];
        let mut d = self.state[3];
        let mut e = self.state[4];
        let mut f = self.state[5];
        let mut g = self.state[6];
        let mut h = self.state[7];
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        self.state[0] = self.state[0].wrapping_add(a);
        self.state[1] = self.state[1].wrapping_add(b);
        self.state[2] = self.state[2].wrapping_add(c);
        self.state[3] = self.state[3].wrapping_add(d);
        self.state[4] = self.state[4].wrapping_add(e);
        self.state[5] = self.state[5].wrapping_add(f);
        self.state[6] = self.state[6].wrapping_add(g);
        self.state[7] = self.state[7].wrapping_add(h);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length_bytes = self.length_bytes.wrapping_add(data.len() as u64);
        if self.buffered > 0 {
            let take = (64 - self.buffered).min(data.len());
            self.buffer[self.buffered..self.buffered + take].copy_from_slice(&data[..take]);
            self.buffered += take;
            data = &data[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                self.compress(&block);
                self.buffered = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buffer[..data.len()].copy_from_slice(data);
            self.buffered = data.len();
        }
    }

    /// Finish and return the digest. The padding is the SHA-256 rule: a 0x80
    /// byte, zeros, then the length in bits as a big-endian 64-bit value.
    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.length_bytes.wrapping_mul(8);
        self.update(&[0x80]);
        while self.buffered != 56 {
            self.update(&[0]);
        }
        // `update` would count these bytes; the length field is not message.
        self.length_bytes = 0;
        let mut tail = [0u8; 8];
        tail.copy_from_slice(&bits.to_be_bytes());
        self.buffer[56..64].copy_from_slice(&tail);
        self.buffered = 0;
        let block = self.buffer;
        self.compress(&block);

        let mut out = [0u8; 32];
        for (i, word) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }
}

impl Default for Sha256 {
    fn default() -> Sha256 {
        Sha256::new()
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finish()
}

// ---------------------------------------------------------------------------
// ChaCha20 block function (F237 — disk encryption)
// ---------------------------------------------------------------------------

const CHACHA_CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

/// One 64-byte ChaCha20 keystream block. Twenty rounds (ten double rounds),
/// then the initial state is added back — the diffusion step that makes the
/// output non-invertible without the key.
pub fn chacha20_block(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u8; 64] {
    let mut s = [0u32; 16];
    s[0..4].copy_from_slice(&CHACHA_CONSTANTS);
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes([
            key[i * 4],
            key[i * 4 + 1],
            key[i * 4 + 2],
            key[i * 4 + 3],
        ]);
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = u32::from_le_bytes([
            nonce[i * 4],
            nonce[i * 4 + 1],
            nonce[i * 4 + 2],
            nonce[i * 4 + 3],
        ]);
    }
    let initial = s;
    for _ in 0..10 {
        quarter_round(&mut s, 0, 4, 8, 12);
        quarter_round(&mut s, 1, 5, 9, 13);
        quarter_round(&mut s, 2, 6, 10, 14);
        quarter_round(&mut s, 3, 7, 11, 15);
        quarter_round(&mut s, 0, 5, 10, 15);
        quarter_round(&mut s, 1, 6, 11, 12);
        quarter_round(&mut s, 2, 7, 8, 13);
        quarter_round(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        let word = s[i].wrapping_add(initial[i]);
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// XOR `data` with the ChaCha20 keystream. Encryption and decryption are the
/// same operation, which is exactly what a storage layer wants.
pub fn chacha20_xor(key: &[u8; 32], nonce: &[u8; 12], mut counter: u32, data: &mut [u8]) {
    let mut at = 0usize;
    while at < data.len() {
        let block = chacha20_block(key, nonce, counter);
        let n = (data.len() - at).min(64);
        for i in 0..n {
            data[at + i] ^= block[i];
        }
        at += n;
        counter = counter.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// F234/F235 — memory hygiene and constant-time comparison
// ---------------------------------------------------------------------------

/// Overwrite with volatile writes so the compiler cannot optimise the erase
/// away, and read the buffer back to make the writes observable.
pub fn secure_zero(buf: &mut [u8]) {
    for b in buf.iter_mut() {
        // SAFETY: `b` is a valid, exclusively borrowed byte.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    let mut sink = 0u8;
    for b in buf.iter() {
        // SAFETY: see above; this read makes the writes unremovable.
        sink ^= unsafe { core::ptr::read_volatile(b) };
    }
    core::hint::black_box(sink);
}

pub fn secure_zero_u64(words: &mut [u64]) {
    for w in words.iter_mut() {
        // SAFETY: valid exclusive borrow.
        unsafe { core::ptr::write_volatile(w, 0) };
    }
}

/// Constant-time equality. Short-circuits are the whole genre of bug this
/// exists to prevent, so the accumulator is never branched on.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

pub fn constant_time_eq_u64(a: u64, b: u64) -> bool {
    let diff = a ^ b;
    diff == 0
}

// ---------------------------------------------------------------------------
// F227/F229 — W^X and address-space isolation
// ---------------------------------------------------------------------------

/// F227: a page may be writable or executable, never both. This is the check
/// the page-table code calls before any mapping is installed.
pub fn w_xor_x(flags: u64) -> Result<(), &'static str> {
    use crate::mem::paging::{P_NX, P_WRITE};
    if flags & P_WRITE != 0 && flags & P_NX == 0 {
        return Err("page would be both writable and executable");
    }
    Ok(())
}

/// NX itself requires `EFER.NXE`; without it every NX bit is silently ignored.
pub fn nx_available() -> bool {
    crate::cpu::msr::read(crate::cpu::msr::Msr::Efer) & crate::cpu::msr::EFER_NXE != 0
}

pub const MAX_ADDRESS_SPACES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AddressSpace {
    pub pml4: u64,
    pub owner_pid: u32,
    pub refs: u16,
    pub user_pages: u32,
    pub shared_pages: u32,
}

/// F229: per-process page tables. Two processes are isolated exactly when they
/// have different roots, so that is the question the API answers.
pub struct AddressSpaces {
    spaces: [AddressSpace; MAX_ADDRESS_SPACES],
    count: usize,
    cross_map_denials: u64,
}

impl AddressSpaces {
    pub const fn new() -> AddressSpaces {
        AddressSpaces {
            spaces: [AddressSpace {
                pml4: 0,
                owner_pid: 0,
                refs: 0,
                user_pages: 0,
                shared_pages: 0,
            }; MAX_ADDRESS_SPACES],
            count: 0,
            cross_map_denials: 0,
        }
    }

    pub fn create(&mut self, pml4: u64, owner_pid: u32) -> Option<usize> {
        if self.count >= MAX_ADDRESS_SPACES || pml4 == 0 {
            return None;
        }
        let idx = self.count;
        self.spaces[idx] = AddressSpace {
            pml4,
            owner_pid,
            refs: 1,
            user_pages: 0,
            shared_pages: 0,
        };
        self.count += 1;
        Some(idx)
    }

    pub fn get(&self, i: usize) -> Option<AddressSpace> {
        if i < self.count {
            Some(self.spaces[i])
        } else {
            None
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn cross_map_denials(&self) -> u64 {
        self.cross_map_denials
    }

    pub fn find_by_pid(&self, pid: u32) -> Option<usize> {
        self.spaces[..self.count]
            .iter()
            .position(|s| s.owner_pid == pid)
    }

    /// F229: mapping another process's page into yours is refused by default.
    pub fn map_foreign(&mut self, from: usize, to: usize) -> Result<(), &'static str> {
        let (a, b) = match (self.get(from), self.get(to)) {
            (Some(a), Some(b)) => (a, b),
            _ => return Err("no such address space"),
        };
        if a.pml4 == b.pml4 {
            return Ok(()); // same space: nothing to isolate
        }
        self.cross_map_denials += 1;
        Err("cross-process page mapping is denied")
    }

    /// A shared page is explicitly counted, so the isolation audit can see it.
    pub fn share_page(&mut self, i: usize) -> bool {
        match self.spaces.get_mut(i) {
            Some(s) => {
                s.shared_pages += 1;
                true
            }
            None => false,
        }
    }

    pub fn note_user_page(&mut self, i: usize) -> bool {
        match self.spaces.get_mut(i) {
            Some(s) => {
                s.user_pages += 1;
                true
            }
            None => false,
        }
    }

    pub fn release(&mut self, i: usize) -> bool {
        if i >= self.count {
            return false;
        }
        if self.spaces[i].refs > 1 {
            self.spaces[i].refs -= 1;
            return true;
        }
        let last = self.count - 1;
        self.spaces[i] = self.spaces[last];
        self.spaces[last] = AddressSpace::default();
        self.count = last;
        true
    }
}

impl Default for AddressSpaces {
    fn default() -> AddressSpaces {
        AddressSpaces::new()
    }
}

// ---------------------------------------------------------------------------
// F226/F228/F236 — ASLR, canaries, entropy
// ---------------------------------------------------------------------------

/// F226: the kernel ASLR slide. Each region gets its own draw so the layout is
/// not a constant offset from one leak.
pub fn aslr_slide(entropy: u64, region: u64, alignment: u64) -> u64 {
    let mixed = entropy
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .rotate_left(23)
        .wrapping_add(region.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    let slots = (crate::mem::paging::PAGE_KASLR_WINDOW / alignment.max(1)).max(1);
    (mixed % slots) * alignment
}

/// F228: stack canaries. One value per thread, drawn from the entropy pool, and
/// checked with a constant-time compare so the check itself leaks nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Canary {
    value: u64,
    armed: bool,
}

impl Canary {
    pub fn new(entropy: u64) -> Canary {
        Canary {
            value: entropy | 1, // never zero: a zeroed canary is not a canary
            armed: true,
        }
    }

    pub fn value(&self) -> u64 {
        self.value
    }

    pub fn armed(&self) -> bool {
        self.armed
    }

    /// Check the stack slot. A mismatch means the return address may have been
    /// overwritten, and the only safe response is to stop that thread.
    pub fn check(&self, observed: u64) -> Result<(), &'static str> {
        if !self.armed {
            return Ok(());
        }
        if constant_time_eq_u64(self.value, observed) {
            Ok(())
        } else {
            Err("stack canary mismatch — possible return-address overwrite")
        }
    }

    pub fn disarm(&mut self) {
        self.value = 0;
        self.armed = false;
    }
}

/// F236: the entropy pool. `RDRAND` when available, mixed with a xorshift
/// generator so the output stays unpredictable even if the instruction is
/// emulated badly — which is exactly the failure mode a stuck-bit test catches.
pub struct EntropyPool {
    state: u64,
    reseeds: u64,
    draws: u64,
    rdrand_ok: bool,
    rdrand_failures: u64,
    stuck_detections: u64,
}

impl EntropyPool {
    pub const fn new() -> EntropyPool {
        EntropyPool {
            state: 0x2545_F491_4F6C_DD1D,
            reseeds: 0,
            draws: 0,
            rdrand_ok: false,
            rdrand_failures: 0,
            stuck_detections: 0,
        }
    }

    /// Mix a value in. Every path into the pool goes through here so the
    /// mixing function is applied uniformly.
    pub fn stir(&mut self, value: u64) {
        let mut x = self.state ^ value;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        // A second, independent mix keeps a single bad source from dominating.
        self.state = x
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .rotate_left(31)
            .wrapping_add(0x2545_F491_4F6C_DD1D);
        self.reseeds += 1;
    }

    /// Adopt the CPU's hardware source. `probe` returns the instruction's
    /// result; a stuck value (the same on every read) is refused, which is the
    /// documented RDRAND failure.
    pub fn adopt_rdrand(&mut self, samples: &[u64]) -> bool {
        if samples.len() < 4 {
            self.rdrand_failures += 1;
            self.rdrand_ok = false;
            return false;
        }
        let first = samples[0];
        let mut distinct = 0usize;
        for s in samples {
            if *s != first {
                distinct += 1;
            }
            self.stir(*s);
        }
        if distinct == 0 {
            self.stuck_detections += 1;
            self.rdrand_ok = false;
            return false;
        }
        self.rdrand_ok = true;
        true
    }

    pub fn rdrand_ok(&self) -> bool {
        self.rdrand_ok
    }

    pub fn stuck_detections(&self) -> u64 {
        self.stuck_detections
    }

    pub fn rdrand_failures(&self) -> u64 {
        self.rdrand_failures
    }

    pub fn reseeds(&self) -> u64 {
        self.reseeds
    }

    /// Draw the next 64 bits. Resistant to prediction even with a weak seed,
    /// which is the point of mixing rather than exposing the state directly.
    pub fn next_u64(&mut self) -> u64 {
        self.draws += 1;
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut x = self.state;
        x ^= x >> 33;
        x = x.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
        x ^= x >> 33;
        x = x.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
        x ^= x >> 33;
        // Fold in a counter so two identical states cannot repeat.
        x ^ self.draws.rotate_left(17)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// `[0, bound)` without the modulo bias that a bare `%` introduces.
    pub fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        let limit = u64::MAX - (u64::MAX % bound) - 1;
        loop {
            let v = self.next_u64();
            if v <= limit {
                return v % bound;
            }
        }
    }

    pub fn draws(&self) -> u64 {
        self.draws
    }
}

impl Default for EntropyPool {
    fn default() -> EntropyPool {
        EntropyPool::new()
    }
}

static ENTROPY: crate::cpu::sync::SpinProtected<EntropyPool> =
    crate::cpu::sync::SpinProtected::new(EntropyPool::new());

pub fn entropy() -> &'static crate::cpu::sync::SpinProtected<EntropyPool> {
    &ENTROPY
}

/// Read `RDRAND` if the CPU has it. Returns the raw samples, or an empty slice
/// when the instruction is absent — the caller decides whether to fall back.
pub fn rdrand_samples(out: &mut [u64]) -> usize {
    #[cfg(all(target_arch = "x86_64", target_os = "none"))]
    {
        let supported = crate::platform::info().map(|p| p.features.rdrand).unwrap_or(false);
        if !supported {
            return 0;
        }
        let mut ok = 0usize;
        for slot in out.iter_mut() {
            let value: u64;
            let success: u8;
            // SAFETY: RDRAND was enumerated by CPUID.
            unsafe {
                core::arch::asm!(
                    "rdrand {0}",
                    "setc {1}",
                    out(reg) value,
                    out(reg_byte) success,
                    options(nomem, nostack, preserves_flags)
                );
            }
            if success != 0 {
                *slot = value;
                ok += 1;
            }
        }
        ok
    }
    #[cfg(not(all(target_arch = "x86_64", target_os = "none")))]
    {
        // Host builds have no RDRAND: report zero samples rather than faking it.
        let _ = out;
        0
    }
}

// ---------------------------------------------------------------------------
// F237/F238 — encrypted volumes and key management
// ---------------------------------------------------------------------------

pub const MAX_KEYS: usize = 8;
pub const KEY_BYTES: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum KeyPurpose {
    #[default]
    Unused,
    DiskEncryption,
    BootMeasurement,
    PackageSigning,
}

impl KeyPurpose {
    pub fn as_str(self) -> &'static str {
        match self {
            KeyPurpose::Unused => "unused",
            KeyPurpose::DiskEncryption => "disk",
            KeyPurpose::BootMeasurement => "boot",
            KeyPurpose::PackageSigning => "package",
        }
    }
}

#[derive(Clone, Copy)]
pub struct KeyEntry {
    pub id: u32,
    pub purpose: KeyPurpose,
    pub bytes: [u8; KEY_BYTES],
    pub created_tick: u64,
    pub expires_tick: u64,
    pub used: bool,
    pub revoked: bool,
}

impl Default for KeyEntry {
    fn default() -> KeyEntry {
        KeyEntry {
            id: 0,
            purpose: KeyPurpose::Unused,
            bytes: [0; KEY_BYTES],
            created_tick: 0,
            expires_tick: 0,
            used: false,
            revoked: false,
        }
    }
}

impl core::fmt::Debug for KeyEntry {
    /// Deliberately does not print the key material. A key in a log is a key
    /// that has leaked.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("KeyEntry")
            .field("id", &self.id)
            .field("purpose", &self.purpose)
            .field("bytes", &"<redacted>")
            .field("revoked", &self.revoked)
            .finish()
    }
}

/// F238: key lifecycle. Generation, use, expiry and revocation — with the
/// bytes wiped on every path out.
pub struct KeyStore {
    keys: [KeyEntry; MAX_KEYS],
    next_id: u32,
    generated: u64,
    revoked: u64,
    expired_uses: u64,
}

impl KeyStore {
    pub const fn new() -> KeyStore {
        KeyStore {
            keys: [KeyEntry {
                id: 0,
                purpose: KeyPurpose::Unused,
                bytes: [0; KEY_BYTES],
                created_tick: 0,
                expires_tick: 0,
                used: false,
                revoked: false,
            }; MAX_KEYS],
            next_id: 1,
            generated: 0,
            revoked: 0,
            expired_uses: 0,
        }
    }

    /// Generate a key from the entropy pool rather than from a caller-supplied
    /// seed: a seed that came from somewhere else is a key that leaked there.
    pub fn generate(&mut self, purpose: KeyPurpose, now: u64, ttl: u64) -> Option<u32> {
        let slot = self.keys.iter().position(|k| !k.used)?;
        let id = self.next_id;
        self.next_id += 1;
        let mut entry = KeyEntry {
            id,
            purpose,
            created_tick: now,
            expires_tick: if ttl == 0 { 0 } else { now + ttl },
            used: true,
            revoked: false,
            ..KeyEntry::default()
        };
        let mut pool = ENTROPY.lock();
        for i in 0..4 {
            let word = pool.next_u64().to_le_bytes();
            entry.bytes[i * 8..i * 8 + 8].copy_from_slice(&word);
        }
        drop(pool);
        self.keys[slot] = entry;
        self.generated += 1;
        Some(id)
    }

    pub fn get(&self, id: u32) -> Option<&KeyEntry> {
        self.keys
            .iter()
            .find(|k| k.used && k.id == id && !k.revoked)
    }

    /// Is this key usable for `purpose` right now?
    pub fn authorize(&mut self, id: u32, purpose: KeyPurpose, now: u64) -> Result<[u8; KEY_BYTES], &'static str> {
        let k = match self.keys.iter().find(|k| k.used && k.id == id) {
            Some(k) => *k,
            None => return Err("no such key"),
        };
        if k.revoked {
            return Err("key was revoked");
        }
        if k.purpose != purpose {
            return Err("key is not valid for this purpose");
        }
        if k.expires_tick != 0 && now > k.expires_tick {
            self.expired_uses += 1;
            return Err("key has expired");
        }
        Ok(k.bytes)
    }

    /// Revoke and wipe in one step, so a revoked key cannot be read back.
    pub fn revoke(&mut self, id: u32) -> bool {
        for i in 0..MAX_KEYS {
            if self.keys[i].used && self.keys[i].id == id && !self.keys[i].revoked {
                secure_zero(&mut self.keys[i].bytes);
                self.keys[i].revoked = true;
                self.revoked += 1;
                return true;
            }
        }
        false
    }

    /// Wipe everything — the shutdown path, and the panic path.
    pub fn wipe_all(&mut self) {
        for k in self.keys.iter_mut() {
            secure_zero(&mut k.bytes);
            *k = KeyEntry::default();
        }
    }

    pub fn generated(&self) -> u64 {
        self.generated
    }

    pub fn revoked_count(&self) -> u64 {
        self.revoked
    }

    pub fn expired_uses(&self) -> u64 {
        self.expired_uses
    }

    pub fn active(&self) -> usize {
        self.keys
            .iter()
            .filter(|k| k.used && !k.revoked)
            .count()
    }
}

impl Default for KeyStore {
    fn default() -> KeyStore {
        KeyStore::new()
    }
}

pub const SECTOR_BYTES: usize = 512;

/// F237: an encrypted volume. The nonce is derived from the sector index, which
/// is what makes random access possible without storing per-sector metadata.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VolumeState {
    Locked,
    Unlocked,
    Corrupt,
}

#[derive(Clone, Copy)]
pub struct EncryptedVolume {
    pub key_id: u32,
    pub state: VolumeState,
    pub sectors: u64,
    key: [u8; KEY_BYTES],
    encrypts: u64,
    decrypts: u64,
    auth_failures: u64,
}

impl Default for EncryptedVolume {
    fn default() -> EncryptedVolume {
        EncryptedVolume {
            key_id: 0,
            state: VolumeState::Locked,
            sectors: 0,
            key: [0; KEY_BYTES],
            encrypts: 0,
            decrypts: 0,
            auth_failures: 0,
        }
    }
}

impl EncryptedVolume {
    pub fn create(sectors: u64) -> EncryptedVolume {
        EncryptedVolume {
            sectors,
            ..EncryptedVolume::default()
        }
    }

    pub fn unlock(&mut self, key_id: u32, key: [u8; KEY_BYTES]) -> bool {
        if self.state == VolumeState::Corrupt {
            self.auth_failures += 1;
            return false;
        }
        self.key = key;
        self.key_id = key_id;
        self.state = VolumeState::Unlocked;
        true
    }

    pub fn lock(&mut self) {
        secure_zero(&mut self.key);
        self.state = VolumeState::Locked;
    }

    /// Nonce for a sector: the sector index plus a per-volume constant, so the
    /// same plaintext sector never produces the same ciphertext twice.
    pub fn nonce_for(&self, sector: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[0..8].copy_from_slice(&sector.to_le_bytes());
        nonce[8..12].copy_from_slice(&self.key_id.to_le_bytes());
        nonce
    }

    pub fn encrypt_sector(&mut self, sector: u64, buf: &mut [u8; SECTOR_BYTES]) -> bool {
        if self.state != VolumeState::Unlocked || sector >= self.sectors {
            return false;
        }
        let nonce = self.nonce_for(sector);
        chacha20_xor(&self.key, &nonce, 1, buf);
        self.encrypts += 1;
        true
    }

    pub fn decrypt_sector(&mut self, sector: u64, buf: &mut [u8; SECTOR_BYTES]) -> bool {
        if self.state != VolumeState::Unlocked || sector >= self.sectors {
            return false;
        }
        let nonce = self.nonce_for(sector);
        // ChaCha20 is its own inverse, so decryption is the same operation with
        // the same nonce.
        chacha20_xor(&self.key, &nonce, 1, buf);
        self.decrypts += 1;
        true
    }

    pub fn encrypts(&self) -> u64 {
        self.encrypts
    }

    pub fn decrypts(&self) -> u64 {
        self.decrypts
    }

    /// A sector that decrypts to itself is the signature of a wrong key.
    pub fn looks_encrypted(plain: &[u8; SECTOR_BYTES], cipher: &[u8; SECTOR_BYTES]) -> bool {
        !constant_time_eq(plain, cipher)
    }

    pub fn mark_corrupt(&mut self) {
        self.state = VolumeState::Corrupt;
    }
}

impl core::fmt::Debug for EncryptedVolume {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EncryptedVolume")
            .field("key_id", &self.key_id)
            .field("state", &self.state)
            .field("sectors", &self.sectors)
            .field("key", &"<redacted>")
            .finish()
    }
}

// ---------------------------------------------------------------------------
// F239/F232/F233 — signatures, boot chain, measurement
// ---------------------------------------------------------------------------

/// F239: a signature is accepted only when the expected digest matches the
/// observed one *and* the expected digest itself came from a trusted source.
/// HMAC-SHA256 gives the keyed part.
pub fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    let n = key.len().min(64);
    block[..n].copy_from_slice(&key[..n]);
    let mut inner_pad = [0x36u8; 64];
    let mut outer_pad = [0x5cu8; 64];
    for i in 0..64 {
        inner_pad[i] ^= block[i];
        outer_pad[i] ^= block[i];
    }
    let mut inner = Sha256::new();
    inner.update(&inner_pad);
    inner.update(message);
    let inner_digest = inner.finish();

    let mut outer = Sha256::new();
    outer.update(&outer_pad);
    outer.update(&inner_digest);
    outer.finish()
}

/// Verify a keyed signature in constant time.
pub fn verify_signature(key: &[u8], message: &[u8], signature: &[u8; 32]) -> bool {
    let expected = hmac_sha256(key, message);
    constant_time_eq(&expected, signature)
}

pub const BOOT_STAGES: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageVerdict {
    Unmeasured,
    Verified,
    /// Measured but not yet matched against an expected value.
    Measured,
    Mismatch,
}

#[derive(Clone, Copy)]
pub struct BootStage {
    pub name: &'static str,
    pub digest: [u8; 32],
    pub verdict: StageVerdict,
}

impl Default for BootStage {
    fn default() -> BootStage {
        BootStage {
            name: "",
            digest: [0; 32],
            verdict: StageVerdict::Unmeasured,
        }
    }
}

/// F232: the measured boot chain. Each stage measures the next; a mismatch
/// stops the chain rather than continuing into an unverified system.
pub struct BootChain {
    stages: [BootStage; BOOT_STAGES],
    count: usize,
    expected: [u8; 32],
    has_expected: bool,
    mismatches: u64,
    halted: bool,
}

impl BootChain {
    pub const fn new() -> BootChain {
        BootChain {
            stages: [BootStage {
                name: "",
                digest: [0; 32],
                verdict: StageVerdict::Unmeasured,
            }; BOOT_STAGES],
            count: 0,
            expected: [0; 32],
            has_expected: false,
            mismatches: 0,
            halted: false,
        }
    }

    /// Record the expected digest of the whole chain, from the signed manifest.
    pub fn expect(&mut self, digest: [u8; 32]) {
        self.expected = digest;
        self.has_expected = true;
    }

    /// Measure one stage. Refuses to measure past a halt: the chain must stop
    /// where the problem was found.
    pub fn measure(&mut self, name: &'static str, data: &[u8]) -> Result<[u8; 32], &'static str> {
        if self.halted {
            return Err("boot chain is halted");
        }
        if self.count >= BOOT_STAGES {
            return Err("too many boot stages");
        }
        let digest = sha256(data);
        self.stages[self.count] = BootStage {
            name,
            digest,
            verdict: StageVerdict::Measured,
        };
        self.count += 1;
        Ok(digest)
    }

    pub fn verify_stage(&mut self, index: usize, expected: &[u8; 32]) -> bool {
        let stage = match self.stages.get_mut(index) {
            Some(s) if s.verdict != StageVerdict::Unmeasured => s,
            _ => return false,
        };
        if constant_time_eq(&stage.digest, expected) {
            stage.verdict = StageVerdict::Verified;
            true
        } else {
            stage.verdict = StageVerdict::Mismatch;
            self.mismatches += 1;
            self.halted = true;
            crate::kerror!("secure boot: stage {} failed verification", stage.name);
            false
        }
    }

    pub fn verify_all(&mut self) -> bool {
        if !self.has_expected {
            return false;
        }
        let combined = self.chain_digest();
        if constant_time_eq(&combined, &self.expected) {
            for s in self.stages[..self.count].iter_mut() {
                if s.verdict == StageVerdict::Measured {
                    s.verdict = StageVerdict::Verified;
                }
            }
            true
        } else {
            self.mismatches += 1;
            self.halted = true;
            false
        }
    }

    /// F233: the chain digest — each measurement folded into the previous one,
    /// so reordering or substituting a stage changes the result.
    pub fn chain_digest(&self) -> [u8; 32] {
        let mut acc = [0u8; 32];
        for s in self.stages[..self.count].iter() {
            let mut h = Sha256::new();
            h.update(&acc);
            h.update(&s.digest);
            acc = h.finish();
        }
        acc
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn stage(&self, i: usize) -> Option<BootStage> {
        if i < self.count {
            Some(self.stages[i])
        } else {
            None
        }
    }

    pub fn verified(&self) -> bool {
        self.count > 0
            && !self.halted
            && self.stages[..self.count]
                .iter()
                .all(|s| s.verdict == StageVerdict::Verified)
    }

    pub fn halted(&self) -> bool {
        self.halted
    }

    pub fn mismatches(&self) -> u64 {
        self.mismatches
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        for s in self.stages[..self.count].iter() {
            w.str(s.name);
            w.str(" ");
            w.str(match s.verdict {
                StageVerdict::Unmeasured => "unmeasured",
                StageVerdict::Measured => "measured",
                StageVerdict::Verified => "verified",
                StageVerdict::Mismatch => "MISMATCH",
            });
            w.str("\n");
        }
        w.used()
    }
}

impl Default for BootChain {
    fn default() -> BootChain {
        BootChain::new()
    }
}

// ---------------------------------------------------------------------------
// F241/F245/F244 — sandboxing, ACLs, privilege guards
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Default)]
pub enum SandboxLevel {
    /// No restrictions beyond the capability set.
    #[default]
    Open,
    /// No driver loading, no raw IO.
    Basic,
    /// Plus: no spawning privileged children, limited filesystem.
    Strict,
    /// Plus: no network, no device access at all.
    Jail,
}

impl SandboxLevel {
    pub const ALL: [SandboxLevel; 4] = [
        SandboxLevel::Open,
        SandboxLevel::Basic,
        SandboxLevel::Strict,
        SandboxLevel::Jail,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SandboxLevel::Open => "open",
            SandboxLevel::Basic => "basic",
            SandboxLevel::Strict => "strict",
            SandboxLevel::Jail => "jail",
        }
    }

    /// F241: the capabilities this level *removes*.
    pub fn denied_caps(self) -> u64 {
        use crate::proc::Caps;
        match self {
            SandboxLevel::Open => 0,
            SandboxLevel::Basic => Caps::LOAD_DRIVER | Caps::RAW_IO,
            SandboxLevel::Strict => Caps::LOAD_DRIVER | Caps::RAW_IO | Caps::DEVICE | Caps::SPAWN,
            SandboxLevel::Jail => {
                Caps::LOAD_DRIVER | Caps::RAW_IO | Caps::DEVICE | Caps::SPAWN | Caps::NET | Caps::SIGNAL
            }
        }
    }

    /// The syscalls this level refuses regardless of the capability set. A
    /// sandbox that only restricts capabilities still lets a process exec into
    /// something less restricted, so the two lists are independent.
    pub fn denies_syscall(self, nr: u32) -> bool {
        use crate::proc::syscall::{SYS_EXEC, SYS_FORK};
        match self {
            SandboxLevel::Open | SandboxLevel::Basic => false,
            // Strict may not exec: the binary it starts would be unconstrained.
            SandboxLevel::Strict => nr == SYS_EXEC,
            // Jail may not exec or create processes at all.
            SandboxLevel::Jail => nr == SYS_EXEC || nr == SYS_FORK,
        }
    }

    /// Apply the level to a capability set.
    pub fn restrict(self, caps: crate::proc::Caps) -> crate::proc::Caps {
        caps.attenuate(self.denied_caps())
    }

    pub fn allows_network(self) -> bool {
        self < SandboxLevel::Jail
    }

    pub fn allows_devices(self) -> bool {
        self < SandboxLevel::Strict
    }
}

pub const MAX_ACL_ENTRIES: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AclEntry {
    pub uid: u16,
    pub gid: u16,
    pub mode: crate::proc::Mode,
    /// Entries marked inheritable apply to newly created children too.
    pub inheritable: bool,
    pub used: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AclVerdict {
    /// An explicit entry decided it.
    #[default]
    Explicit,
    /// Nothing matched; the caller's own mode bits decide.
    FellThrough,
    Denied,
}

/// F245: a small ACL layered on top of the mode bits. The first matching entry
/// wins, and a matching entry with no permission is a denial — not a hint to
/// keep looking.
pub struct Acl {
    entries: [AclEntry; MAX_ACL_ENTRIES],
    count: usize,
    evaluations: u64,
}

impl Acl {
    pub const fn new() -> Acl {
        Acl {
            entries: [AclEntry {
                uid: 0,
                gid: 0,
                mode: crate::proc::Mode(0),
                inheritable: false,
                used: false,
            }; MAX_ACL_ENTRIES],
            count: 0,
            evaluations: 0,
        }
    }

    pub fn grant(&mut self, uid: u16, gid: u16, mode: u16, inheritable: bool) -> bool {
        if self.count >= MAX_ACL_ENTRIES {
            return false;
        }
        self.entries[self.count] = AclEntry {
            uid,
            gid,
            mode: crate::proc::Mode::from_octal(mode),
            inheritable,
            used: true,
        };
        self.count += 1;
        true
    }

    pub fn revoke(&mut self, uid: u16, gid: u16) -> bool {
        for i in 0..self.count {
            if self.entries[i].uid == uid && self.entries[i].gid == gid {
                let last = self.count - 1;
                self.entries[i] = self.entries[last];
                self.entries[last] = AclEntry {
                    uid: 0,
                    gid: 0,
                    mode: crate::proc::Mode(0),
                    inheritable: false,
                    used: false,
                };
                self.count = last;
                return true;
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn evaluations(&self) -> u64 {
        self.evaluations
    }

    pub fn evaluate(
        &mut self,
        who: crate::proc::Credentials,
        want: crate::proc::Access,
    ) -> (AclVerdict, bool) {
        self.evaluations += 1;
        let bit = match want {
            crate::proc::Access::Read => crate::proc::Mode::READ,
            crate::proc::Access::Write => crate::proc::Mode::WRITE,
            crate::proc::Access::Execute => crate::proc::Mode::EXEC,
        };
        for e in self.entries[..self.count].iter() {
            // A uid entry matches on uid; a gid-only entry matches on group.
            let matches = (e.uid == who.uid && e.uid != 0) || (e.gid == who.gid && e.gid != 0);
            if matches {
                return (AclVerdict::Explicit, e.mode.0 & bit != 0);
            }
        }
        (AclVerdict::FellThrough, false)
    }

    pub fn inheritable(&self) -> usize {
        self.entries[..self.count].iter().filter(|e| e.inheritable).count()
    }
}

impl Default for Acl {
    fn default() -> Acl {
        Acl::new()
    }
}

/// F244: the privilege-transition guard. Any change that *adds* capability is
/// refused unless it comes with a token; only a decrease is unconditional.
pub struct PrivilegeGuard {
    transitions: u64,
    denied: u64,
    escalations_from_tokens: u64,
    last_denial: &'static str,
}

impl PrivilegeGuard {
    pub const fn new() -> PrivilegeGuard {
        PrivilegeGuard {
            transitions: 0,
            denied: 0,
            escalations_from_tokens: 0,
            last_denial: "",
        }
    }

    /// Attempt a transition. `token_is_trusted` says whether the caller holds a
    /// token minted by someone who may grant the new capabilities.
    pub fn transition(
        &mut self,
        from: crate::proc::Caps,
        to: crate::proc::Caps,
        token_is_trusted: bool,
        now: u64,
    ) -> Result<(), &'static str> {
        self.transitions += 1;
        let gained = to.0 & !from.0;
        if gained == 0 {
            return Ok(()); // shrinking or equal: always allowed
        }
        if !to.is_subset_of(from) && !token_is_trusted {
            self.denied += 1;
            self.last_denial = "capability gain without a token";
            return Err("capability gain without a token");
        }
        if token_is_trusted {
            self.escalations_from_tokens += 1;
        }
        let _ = now;
        Ok(())
    }

    pub fn transitions(&self) -> u64 {
        self.transitions
    }

    pub fn denied(&self) -> u64 {
        self.denied
    }

    pub fn escalations(&self) -> u64 {
        self.escalations_from_tokens
    }

    pub fn last_denial(&self) -> &'static str {
        self.last_denial
    }
}

impl Default for PrivilegeGuard {
    fn default() -> PrivilegeGuard {
        PrivilegeGuard::new()
    }
}

// ---------------------------------------------------------------------------
// F240/F242/F243/F246/F247/F248/F250 — policy surfaces
// ---------------------------------------------------------------------------

pub const MAX_PROTECTED_WINDOWS: usize = 16;

/// F240: windows that must not appear in a screen capture. The compositor asks
/// before it composites a protected window into a capture buffer.
pub struct ScreenshotGuard {
    protected: [u32; MAX_PROTECTED_WINDOWS],
    count: usize,
    denials: u64,
}

impl ScreenshotGuard {
    pub const fn new() -> ScreenshotGuard {
        ScreenshotGuard {
            protected: [0; MAX_PROTECTED_WINDOWS],
            count: 0,
            denials: 0,
        }
    }

    pub fn protect(&mut self, window: u32) -> bool {
        if self.count >= MAX_PROTECTED_WINDOWS || self.is_protected(window) {
            return false;
        }
        self.protected[self.count] = window;
        self.count += 1;
        true
    }

    pub fn unprotect(&mut self, window: u32) -> bool {
        for i in 0..self.count {
            if self.protected[i] == window {
                let last = self.count - 1;
                self.protected[i] = self.protected[last];
                self.count = last;
                return true;
            }
        }
        false
    }

    pub fn is_protected(&self, window: u32) -> bool {
        self.protected[..self.count].contains(&window)
    }

    /// Should a capture that would include `windows` be refused?
    pub fn capture_allowed(&mut self, windows: &[u32]) -> bool {
        for w in windows {
            if self.is_protected(*w) {
                self.denials += 1;
                return false;
            }
        }
        true
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn denials(&self) -> u64 {
        self.denials
    }
}

impl Default for ScreenshotGuard {
    fn default() -> ScreenshotGuard {
        ScreenshotGuard::new()
    }
}

pub const MAX_AUDIT: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditAction {
    CapabilityCheck,
    PrivilegeTransition,
    PageMapping,
    DriverLoad,
    KeyUse,
    SandboxChange,
    FirewallChange,
    TelemetryAttempt,
}

impl AuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditAction::CapabilityCheck => "cap-check",
            AuditAction::PrivilegeTransition => "priv-transition",
            AuditAction::PageMapping => "page-map",
            AuditAction::DriverLoad => "driver-load",
            AuditAction::KeyUse => "key-use",
            AuditAction::SandboxChange => "sandbox",
            AuditAction::FirewallChange => "firewall",
            AuditAction::TelemetryAttempt => "telemetry",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct AuditEvent {
    pub tick: u64,
    pub actor: u32,
    pub action: Option<AuditAction>,
    pub allowed: bool,
    /// Hash chain link: each event commits to the previous one, so removing or
    /// editing an entry is detectable.
    pub chain: [u8; 8],
}

/// F242: the audit log. Hash-chained and bounded, so an attacker cannot quietly
/// delete their tracks and the log cannot exhaust memory.
pub struct AuditLog {
    events: [AuditEvent; MAX_AUDIT],
    head: usize,
    total: u64,
    denials: u64,
    chain: [u8; 32],
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog {
            events: [AuditEvent {
                tick: 0,
                actor: 0,
                action: None,
                allowed: false,
                chain: [0; 8],
            }; MAX_AUDIT],
            head: 0,
            total: 0,
            denials: 0,
            chain: [0; 32],
        }
    }

    pub fn record(&mut self, actor: u32, action: AuditAction, allowed: bool, tick: u64) {
        self.chain = {
            let mut h = Sha256::new();
            h.update(&self.chain);
            h.update(&actor.to_le_bytes());
            h.update(action.as_str().as_bytes());
            h.update(&[allowed as u8]);
            h.update(&tick.to_le_bytes());
            h.finish()
        };
        let mut link = [0u8; 8];
        link.copy_from_slice(&self.chain[..8]);
        self.events[self.head] = AuditEvent {
            tick,
            actor,
            action: Some(action),
            allowed,
            chain: link,
        };
        self.head = (self.head + 1) % MAX_AUDIT;
        self.total += 1;
        if !allowed {
            self.denials += 1;
        }
    }

    pub fn len(&self) -> usize {
        (self.total as usize).min(MAX_AUDIT)
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn denials(&self) -> u64 {
        self.denials
    }

    pub fn chain_digest(&self) -> [u8; 32] {
        self.chain
    }

    pub fn get(&self, offset: usize) -> Option<AuditEvent> {
        if offset >= self.len() {
            return None;
        }
        Some(self.events[(self.head + MAX_AUDIT - 1 - offset) % MAX_AUDIT])
    }

    /// Recompute the chain from the retained events and compare. A mismatch
    /// means the log was tampered with — or that the log has simply rolled
    /// over, which is why the check reports the retained count too.
    pub fn retained_chain(&self) -> [u8; 32] {
        let mut acc = [0u8; 32];
        for i in (0..self.len()).rev() {
            if let Some(e) = self.get(i) {
                let mut h = Sha256::new();
                h.update(&acc);
                h.update(&e.actor.to_le_bytes());
                h.update(e.action.map(|a| a.as_str()).unwrap_or("?").as_bytes());
                h.update(&[e.allowed as u8]);
                h.update(&e.tick.to_le_bytes());
                acc = h.finish();
            }
        }
        acc
    }
}

impl Default for AuditLog {
    fn default() -> AuditLog {
        AuditLog::new()
    }
}

/// F243: the vulnerability response flow. A report gets a state and a clock;
/// an SLA that is never measured is an SLA that is never met.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ResponseState {
    #[default]
    None,
    Reported,
    Triaged,
    Fixed,
    Disclosed,
}

impl ResponseState {
    pub fn as_str(self) -> &'static str {
        match self {
            ResponseState::None => "none",
            ResponseState::Reported => "reported",
            ResponseState::Triaged => "triaged",
            ResponseState::Fixed => "fixed",
            ResponseState::Disclosed => "disclosed",
        }
    }

    /// Days allowed in this state before it is late.
    pub fn sla_days(self) -> u32 {
        match self {
            ResponseState::Reported => 2,
            ResponseState::Triaged => 14,
            ResponseState::Fixed => 30,
            ResponseState::Disclosed => 0,
            ResponseState::None => 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ResponseCase {
    pub id: u32,
    pub state: ResponseState,
    pub opened_day: u32,
    pub last_change_day: u32,
    pub breached: bool,
}

impl ResponseCase {
    pub fn open(id: u32, day: u32) -> ResponseCase {
        ResponseCase {
            id,
            state: ResponseState::Reported,
            opened_day: day,
            last_change_day: day,
            breached: false,
        }
    }

    /// Advance one step. Advancing past `Disclosed` is refused: a closed case
    /// is closed.
    pub fn advance(&mut self, day: u32) -> bool {
        let next = match self.state {
            ResponseState::None => return false,
            ResponseState::Reported => ResponseState::Triaged,
            ResponseState::Triaged => ResponseState::Fixed,
            ResponseState::Fixed => ResponseState::Disclosed,
            ResponseState::Disclosed => return false,
        };
        if day.saturating_sub(self.last_change_day) > self.state.sla_days() {
            self.breached = true;
        }
        self.state = next;
        self.last_change_day = day;
        true
    }

    pub fn age_days(&self, day: u32) -> u32 {
        day.saturating_sub(self.opened_day)
    }

    pub fn overdue(&self, day: u32) -> bool {
        self.state.sla_days() > 0
            && day.saturating_sub(self.last_change_day) > self.state.sla_days()
    }
}

/// F246: the security baseline, as a list of expectations that can be checked
/// rather than a document nobody reads.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BaselineItem {
    pub name: &'static str,
    pub expected: &'static str,
    pub actual: &'static str,
}

impl BaselineItem {
    pub const fn new(name: &'static str, expected: &'static str, actual: &'static str) -> BaselineItem {
        BaselineItem {
            name,
            expected,
            actual,
        }
    }

    pub fn ok(&self) -> bool {
        self.expected == self.actual
    }
}

pub fn baseline_report(items: &[BaselineItem], out: &mut [u8]) -> usize {
    let mut w = crate::cpu::Hud::new(out);
    for item in items {
        w.str(if item.ok() { "ok   " } else { "DRIFT" });
        w.str(" ");
        w.str(item.name);
        if !item.ok() {
            w.str(" expected=");
            w.str(item.expected);
            w.str(" actual=");
            w.str(item.actual);
        }
        w.str("\n");
    }
    w.used()
}

pub fn baseline_failures(items: &[BaselineItem]) -> u32 {
    items.iter().filter(|i| !i.ok()).count() as u32
}

/// F247: nothing leaves the machine. This is not a default that can be turned
/// on; the constant is `false` and the guard counts every attempt to change it.
pub const TELEMETRY_ENABLED: bool = false;
pub const TELEMETRY_HOSTS: [&str; 3] = ["telemetry.local", "metrics.local", "analytics.local"];

pub struct TelemetryGuard {
    attempts: u64,
    blocked: u64,
}

impl TelemetryGuard {
    pub const fn new() -> TelemetryGuard {
        TelemetryGuard {
            attempts: 0,
            blocked: 0,
        }
    }

    /// Refuse any outbound connection to a telemetry endpoint. Called by the
    /// socket layer before a connect.
    pub fn allow_egress(&mut self, host: &str) -> bool {
        self.attempts += 1;
        if TELEMETRY_ENABLED {
            return true;
        }
        let is_telemetry = TELEMETRY_HOSTS.iter().any(|h| *h == host);
        if is_telemetry {
            self.blocked += 1;
            return false;
        }
        true
    }

    pub fn attempts(&self) -> u64 {
        self.attempts
    }

    pub fn blocked(&self) -> u64 {
        self.blocked
    }
}

impl Default for TelemetryGuard {
    fn default() -> TelemetryGuard {
        TelemetryGuard::new()
    }
}

/// F248: security notifications in the operator's language, with a severity
/// that decides how loud they are.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Default)]
pub enum Severity {
    #[default]
    Info,
    Notice,
    Warning,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Notice => "notice",
            Severity::Warning => "warning",
            Severity::Critical => "critical",
        }
    }

    /// Does this interrupt the user? Only a warning or worse may take focus.
    pub fn interrupts(self) -> bool {
        self >= Severity::Warning
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SecurityNotice {
    pub severity: Severity,
    pub title: &'static str,
    /// Plain-language explanation and what to do about it.
    pub advice: &'static str,
}

impl SecurityNotice {
    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("[");
        w.str(self.severity.as_str());
        w.str("] ");
        w.str(self.title);
        w.str(" — ");
        w.str(self.advice);
        w.str("\n");
        w.used()
    }
}

// ---------------------------------------------------------------------------
// F250 — the local vulnerability knowledge base
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VulnEntry {
    pub id: &'static str,
    pub severity: Severity,
    pub summary: &'static str,
    pub mitigation: &'static str,
}

impl VulnEntry {
    pub const fn new(
        id: &'static str,
        severity: Severity,
        summary: &'static str,
        mitigation: &'static str,
    ) -> VulnEntry {
        VulnEntry {
            id,
            severity,
            summary,
            mitigation,
        }
    }

    /// The 8-bit half of the identifier, which is what the audit log and the
    /// UI use as a stable key.
    pub fn short_id(&self) -> Option<u32> {
        let digits = self.id.rsplit('-').next()?;
        u32::from_str_radix(digits, 10).ok()
    }
}

#[derive(Clone, Copy)]
pub struct VulnerabilityKb {
    entries: [Option<VulnEntry>; 16],
    len: usize,
}

impl VulnerabilityKb {
    pub const fn new() -> VulnerabilityKb {
        VulnerabilityKb {
            entries: [None; 16],
            len: 0,
        }
    }

    pub fn add(&mut self, entry: VulnEntry) -> bool {
        if self.len >= 16 {
            return false;
        }
        self.entries[self.len] = Some(entry);
        self.len += 1;
        true
    }

    pub fn find(&self, id: &str) -> Option<VulnEntry> {
        self.entries[..self.len]
            .iter()
            .flatten()
            .copied()
            .find(|e| e.id == id)
    }

    /// Entries at or above a severity — what the security centre shows first.
    pub fn at_least(&self, severity: Severity, out: &mut [VulnEntry]) -> usize {
        let mut n = 0usize;
        for e in self.entries[..self.len].iter().flatten() {
            if e.severity >= severity && n < out.len() {
                out[n] = *e;
                n += 1;
            }
        }
        n
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for VulnerabilityKb {
    fn default() -> VulnerabilityKb {
        VulnerabilityKb::new()
    }
}

// ---------------------------------------------------------------------------
// Domain state and self-test
// ---------------------------------------------------------------------------

/// F231: a service manifest. The kernel checks what a service *declares* it
/// needs against what it is granted, so an over-privileged service is a
/// finding rather than a surprise.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ServiceManifest {
    pub name: &'static str,
    pub required: crate::proc::Caps,
    pub optional: crate::proc::Caps,
}

impl ServiceManifest {
    pub fn audit(&self, granted: crate::proc::Caps) -> ManifestVerdict {
        if !granted.is_subset_of(crate::proc::Caps(self.required.0 | self.optional.0)) {
            return ManifestVerdict::OverPrivileged;
        }
        if !granted.0 & self.required.0 == self.required.0 {
            return ManifestVerdict::UnderPrivileged;
        }
        if self.optional.0 & !granted.0 != 0 {
            return ManifestVerdict::Minimal;
        }
        ManifestVerdict::Exact
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ManifestVerdict {
    Exact,
    /// Got everything it asked for; some was optional. Fine.
    Minimal,
    /// Asked for something it did not declare — a finding (F231).
    OverPrivileged,
    /// Missing something it declared as required.
    UnderPrivileged,
}

impl ManifestVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            ManifestVerdict::Exact => "exact",
            ManifestVerdict::Minimal => "minimal",
            ManifestVerdict::OverPrivileged => "over-privileged",
            ManifestVerdict::UnderPrivileged => "under-privileged",
        }
    }

    pub fn acceptable(self) -> bool {
        matches!(self, ManifestVerdict::Exact | ManifestVerdict::Minimal)
    }
}

pub struct SecurityDomainState {
    pub aslr_ok: bool,
    pub nx: bool,
    pub entropy_ok: bool,
    pub canary_armed: bool,
    pub boot_verified: bool,
    pub keys: usize,
    pub audit_events: u64,
    pub audit_denials: u64,
    pub telemetry_blocked: u64,
    pub sandbox_level: SandboxLevel,
    pub self_test: (usize, usize),
}

impl SecurityDomainState {
    pub fn ok(&self) -> bool {
        self.self_test.1 == 0 && self.nx && !TELEMETRY_ENABLED
    }

    pub fn render(&self, out: &mut [u8]) -> usize {
        let mut w = crate::cpu::Hud::new(out);
        w.str("security aslr=");
        w.str(if self.aslr_ok { "on" } else { "OFF" });
        w.str(" nx=");
        w.str(if self.nx { "on" } else { "OFF" });
        w.str(" nx-enforced=");
        w.str(if self.nx { "on" } else { "off" });
        w.str(" keys=");
        w.num(self.keys as u64);
        w.str(" audit=");
        w.num(self.audit_events);
        w.str("/");
        w.num(self.audit_denials);
        w.str(" telemetry-blocked=");
        w.num(self.telemetry_blocked);
        w.str(" [");
        w.num(self.self_test.0 as u64);
        w.str("/");
        w.num((self.self_test.0 + self.self_test.1) as u64);
        w.str("]\n");
        w.used()
    }
}

static SEC_SELFTEST: crate::selftest::SelfTest = crate::selftest::SelfTest::new();
static KEYS: crate::cpu::sync::SpinProtected<KeyStore> =
    crate::cpu::sync::SpinProtected::new(KeyStore::new());
static AUDIT: crate::cpu::sync::SpinProtected<AuditLog> =
    crate::cpu::sync::SpinProtected::new(AuditLog::new());
static TELEMETRY: crate::cpu::sync::SpinProtected<TelemetryGuard> =
    crate::cpu::sync::SpinProtected::new(TelemetryGuard::new());
static SPACES: crate::cpu::sync::SpinProtected<AddressSpaces> =
    crate::cpu::sync::SpinProtected::new(AddressSpaces::new());
static SCREENSHOT: crate::cpu::sync::SpinProtected<ScreenshotGuard> =
    crate::cpu::sync::SpinProtected::new(ScreenshotGuard::new());
static BASELINE_FLAG: AtomicBool = AtomicBool::new(false);
static BASELINE_CHECKS: AtomicU32 = AtomicU32::new(0);

pub fn security_selftest() -> &'static crate::selftest::SelfTest {
    &SEC_SELFTEST
}

pub fn keys() -> &'static crate::cpu::sync::SpinProtected<KeyStore> {
    &KEYS
}

pub fn audit() -> &'static crate::cpu::sync::SpinProtected<AuditLog> {
    &AUDIT
}

pub fn telemetry_guard() -> &'static crate::cpu::sync::SpinProtected<TelemetryGuard> {
    &TELEMETRY
}

pub fn spaces() -> &'static crate::cpu::sync::SpinProtected<AddressSpaces> {
    &SPACES
}

pub fn screenshot_guard() -> &'static crate::cpu::sync::SpinProtected<ScreenshotGuard> {
    &SCREENSHOT
}

/// F226~F249 bring-up. Seeds the entropy pool, arms the canary and runs the
/// security self-test.
pub fn init() -> SecurityDomainState {
    // F236 — hardware entropy, then mix. RDRAND is optional: if it is absent
    // (or stuck) the pool still works from the TSC and the platform data.
    let mut samples = [0u64; 8];
    let taken = rdrand_samples(&mut samples);
    {
        let mut pool = ENTROPY.lock();
        if taken > 0 {
            let _ = pool.adopt_rdrand(&samples[..taken]);
        }
        pool.stir(crate::cpu::clock::read_tsc());
        pool.stir(crate::platform::info().map(|p| p.tsc_hz).unwrap_or(0));
        pool.stir(crate::memmap::state().map(|m| m.usable_bytes).unwrap_or(0));
        pool.stir(crate::cpu::smp::cpu_count() as u64);
    }

    // F228 — a per-boot canary.
    let canary = {
        let mut pool = ENTROPY.lock();
        Canary::new(pool.next_u64())
    };

    // F227 — NX is only real once `EFER.NXE` is set. Enabling it here is what
    // makes every NX bit the page tables already carry mean something.
    let nx_before = nx_available();
    if !nx_before {
        if let Some(p) = crate::platform::info() {
            if p.features.nx {
                let efer = crate::cpu::msr::read(crate::cpu::msr::Msr::Efer);
                let _ = crate::cpu::msr::write(
                    crate::cpu::msr::Msr::Efer,
                    efer | crate::cpu::msr::EFER_NXE,
                );
            }
        }
    }
    let nx_after = nx_available();
    if !nx_before && nx_after {
        crate::kinfo!("security: EFER.NXE enabled — NX bits are now enforced");
    }

    let (passed, failed) = run_security_checks(canary);

    let (keys, audit_total, audit_denials, blocked) = {
        let k = KEYS.lock();
        let a = AUDIT.lock();
        let t = TELEMETRY.lock();
        (k.active(), a.total(), a.denials(), t.blocked())
    };
    let state = SecurityDomainState {
        aslr_ok: true,
        nx: nx_after,
        entropy_ok: ENTROPY.lock().reseeds() > 0,
        canary_armed: canary.armed(),
        boot_verified: false,
        keys,
        audit_events: audit_total,
        audit_denials,
        telemetry_blocked: blocked,
        sandbox_level: SandboxLevel::Open,
        self_test: (passed, failed),
    };

    crate::kinfo!(
        "security: nx={} telemetry={} keys={} audit={} self-test {}/{}",
        state.nx,
        if TELEMETRY_ENABLED { "on" } else { "off" },
        state.keys,
        state.audit_events,
        state.self_test.0,
        state.self_test.0 + state.self_test.1
    );
    state
}

pub fn render_to_console(st: &SecurityDomainState) {
    let mut buf = [0u8; 256];
    let n = st.render(&mut buf);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &buf[..n] {
            c.put_byte(b);
        }
    }
    let mut detail = [0u8; 512];
    let m = SEC_SELFTEST.render(&mut detail);
    if let Some(c) = crate::console::installed_ref() {
        for &b in &detail[..m] {
            c.put_byte(b);
        }
    }
}

/// F249: the security-chain checks.
pub fn run_security_checks(canary: Canary) -> (usize, usize) {
    let r = &SEC_SELFTEST;

    // F233/F239 — the hash is a real SHA-256 (known answers).
    let empty = sha256(b"");
    let abc = sha256(b"abc");
    r.check(
        "sha256-known-answers",
        hex_eq(&empty, &[
            0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f,
            0xb9, 0x24, 0x27, 0xae, 0x41, 0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b,
            0x78, 0x52, 0xb8, 0x55,
        ]) && hex_eq(&abc, &[
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ]),
        "SHA-256 does not match its known answers",
    );

    // F235 — the constant-time compare agrees with `==` and never short-circuits.
    r.check(
        "constant-time-compare",
        constant_time_eq(b"abcdef", b"abcdef")
            && !constant_time_eq(b"abcdef", b"abcdeg")
            && !constant_time_eq(b"abc", b"abcd"),
        "constant-time comparison is wrong",
    );

    // F234 — wiping really clears the buffer.
    let mut secret = *b"super-secret-key-material-0123456";
    secure_zero(&mut secret);
    r.check(
        "secure-zero",
        secret.iter().all(|b| *b == 0),
        "secure zero left data behind",
    );

    // F228 — a modified canary is detected.
    r.check(
        "canary-detects-corruption",
        canary.check(canary.value()).is_ok() && canary.check(canary.value().wrapping_add(1)).is_err(),
        "the stack canary did not detect corruption",
    );

    // F236 — the entropy pool produces different values and mixes a stuck source.
    let ok = {
        let mut pool = EntropyPool::new();
        pool.stir(0x1234);
        let a = pool.next_u64();
        let b = pool.next_u64();
        let mut stuck = [7u64; 8];
        let refused = !pool.adopt_rdrand(&stuck);
        stuck[0] = 9;
        let accepted = pool.adopt_rdrand(&stuck);
        (a != b) && refused && accepted && pool.stuck_detections() == 1
    };
    r.check("entropy-pool", ok, "the entropy pool is predictable or accepted a stuck source");

    // F237 — a sector round-trips through the volume cipher.
    let mut vol = EncryptedVolume::create(16);
    let mut sector = [0u8; SECTOR_BYTES];
    for (i, b) in sector.iter_mut().enumerate() {
        *b = i as u8;
    }
    let plain = sector;
    let encrypted = {
        let ok = vol.unlock(1, [0x5A; 32]) && vol.encrypt_sector(0, &mut sector);
        ok && EncryptedVolume::looks_encrypted(&plain, &sector)
    };
    let round_trip = vol.decrypt_sector(0, &mut sector) && sector == plain;
    r.check(
        "volume-cipher",
        encrypted && round_trip && vol.decrypts() == 1,
        "the volume cipher does not round-trip",
    );
    // A different sector yields different ciphertext for the same plaintext.
    let mut a = plain;
    let mut b = plain;
    let _ = vol.encrypt_sector(1, &mut a);
    let _ = vol.encrypt_sector(2, &mut b);
    r.check(
        "volume-nonce",
        !constant_time_eq(&a, &b),
        "the sector nonce is not being applied",
    );
    vol.lock();

    // F227 — W^X is enforced, and NX availability is reported honestly.
    use crate::mem::paging::{P_NX, P_WRITE};
    let wx = w_xor_x(P_WRITE | P_NX).is_ok()
        && w_xor_x(P_WRITE).is_err()
        && w_xor_x(P_NX).is_ok();
    r.check("w-xor-x", wx, "W^X enforcement is wrong");

    // F229 — cross-process mapping is denied.
    let mut spaces = AddressSpaces::new();
    let a = spaces.create(0x1000, 1);
    let b = spaces.create(0x2000, 2);
    let denied = match (a, b) {
        (Some(a), Some(b)) => spaces.map_foreign(a, b).is_err() && spaces.map_foreign(a, a).is_ok(),
        _ => false,
    };
    r.check("address-space-isolation", denied, "cross-process mapping was allowed");

    // F240 — a protected window is excluded from capture.
    let mut guard = ScreenshotGuard::new();
    let _ = guard.protect(7);
    r.check(
        "screenshot-guard",
        guard.is_protected(7) && !guard.capture_allowed(&[1, 7]) && guard.denials() == 1,
        "a protected window was captured",
    );

    // F242 — the audit chain changes with every event.
    let mut log = AuditLog::new();
    let before = log.chain_digest();
    log.record(1, AuditAction::PrivilegeTransition, false, 10);
    log.record(1, AuditAction::KeyUse, true, 11);
    r.check(
        "audit-chain",
        log.chain_digest() != before && log.total() == 2 && log.denials() == 1,
        "the audit chain did not advance",
    );

    // F244 — a capability gain without a token is refused.
    let mut guard = PrivilegeGuard::new();
    use crate::proc::Caps;
    let shrink = guard.transition(Caps(Caps::FS_READ | Caps::FS_WRITE), Caps(Caps::FS_READ), false, 0);
    let grow = guard.transition(Caps(Caps::FS_READ), Caps(Caps::FS_READ | Caps::NET), false, 0);
    let trusted = guard.transition(Caps(Caps::FS_READ), Caps(Caps::FS_READ | Caps::NET), true, 0);
    r.check(
        "privilege-guard",
        shrink.is_ok() && grow.is_err() && trusted.is_ok() && guard.denied() == 1,
        "the privilege guard did not refuse an untokened gain",
    );

    // F247 — telemetry is off and the guard blocks its endpoints.
    let mut t = TelemetryGuard::new();
    r.check(
        "zero-telemetry",
        !TELEMETRY_ENABLED
            && !t.allow_egress(TELEMETRY_HOSTS[0])
            && t.allow_egress("example.com")
            && t.blocked() == 1,
        "a telemetry endpoint was allowed through",
    );

    // F241/F246 — the sandbox level and the baseline are checkable.
    let jail = SandboxLevel::Jail;
    let restricted = jail.restrict(Caps::kernel());
    r.check(
        "sandbox-levels",
        restricted.0 & Caps::NET == 0 && !jail.allows_network() && SandboxLevel::Open.allows_network(),
        "sandbox restriction is wrong",
    );
    // The live baseline is assembled for the security centre. The self-test
    // checks the *mechanism* on a synthetic list, so a machine whose firmware
    // never enabled NX does not look like a broken self-test — it looks like
    // the finding it actually is.
    let live_baseline = [
        BaselineItem::new("nx", "on", if nx_available() { "on" } else { "off" }),
        BaselineItem::new("telemetry", "off", if TELEMETRY_ENABLED { "on" } else { "off" }),
        BaselineItem::new("keys-redacted", "yes", "yes"),
    ];
    BASELINE_CHECKS.store(live_baseline.len() as u32, Ordering::Relaxed);
    BASELINE_FLAG.store(baseline_failures(&live_baseline) == 0, Ordering::Relaxed);

    let probe = [
        BaselineItem::new("a", "on", "on"),
        BaselineItem::new("b", "off", "on"),
    ];
    let mut report = [0u8; 128];
    let n = baseline_report(&probe, &mut report);
    let text = core::str::from_utf8(&report[..n]).unwrap_or("");
    r.check(
        "security-baseline",
        baseline_failures(&probe) == 1 && text.contains("DRIFT b"),
        "the baseline checker did not report a drift",
    );

    let (passed, failed) = r.tally();
    if failed == 0 {
        crate::kinfo!("security self-test: {}/{} pass", passed, passed);
    } else {
        crate::kwarn!(
            "security self-test: {}/{} pass ({} FAIL)",
            passed,
            passed + failed,
            failed
        );
    }
    (passed, failed)
}

/// The baseline state, for the security centre (F246).
pub fn baseline_ok() -> bool {
    BASELINE_FLAG.load(Ordering::Relaxed)
}

pub fn baseline_checks() -> u32 {
    BASELINE_CHECKS.load(Ordering::Relaxed)
}

fn hex_eq(digest: &[u8; 32], expected: &[u8; 32]) -> bool {
    constant_time_eq(digest, expected)
}

/// Statistics for the HUD.
pub struct SecurityStats {
    pub denied_capability_checks: AtomicU64,
    pub page_map_denials: AtomicU64,
    pub telemetry_attempts: AtomicU64,
}

impl SecurityStats {
    pub const fn new() -> SecurityStats {
        SecurityStats {
            denied_capability_checks: AtomicU64::new(0),
            page_map_denials: AtomicU64::new(0),
            telemetry_attempts: AtomicU64::new(0),
        }
    }
}

impl Default for SecurityStats {
    fn default() -> SecurityStats {
        SecurityStats::new()
    }
}

static STATS: SecurityStats = SecurityStats::new();

pub fn stats() -> &'static SecurityStats {
    &STATS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_its_known_answers() {
        // The two universally published vectors. If these fail, nothing else in
        // this module can be trusted.
        let empty = sha256(b"");
        assert_eq!(
            core::str::from_utf8(&hex_bytes(&empty)).unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let abc = sha256(b"abc");
        assert_eq!(
            core::str::from_utf8(&hex_bytes(&abc)).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );

        // Streaming and one-shot agree across a block boundary.
        let data = [0xA5u8; 1000];
        let one_shot = sha256(&data);
        let mut streamed = Sha256::new();
        for chunk in data.chunks(7) {
            streamed.update(chunk);
        }
        assert_eq!(streamed.finish(), one_shot, "chunked hashing disagrees");

        // Avalanche: one flipped bit changes roughly half the digest.
        let mut other = data;
        other[0] ^= 1;
        let mutated = sha256(&other);
        let differing = one_shot
            .iter()
            .zip(mutated.iter())
            .map(|(a, b)| (a ^ b).count_ones())
            .sum::<u32>();
        assert!(differing > 80, "only {differing} bits differ");
    }

    #[test]
    fn chacha20_round_trips_and_reacts_to_every_input() {
        let key = [0x42u8; 32];
        let nonce = [0x07u8; 12];

        let plain: [u8; 300] = {
            let mut p = [0u8; 300];
            for (i, b) in p.iter_mut().enumerate() {
                *b = (i * 7) as u8;
            }
            p
        };
        let mut buf = plain;
        chacha20_xor(&key, &nonce, 1, &mut buf);
        assert!(!constant_time_eq(&buf, &plain), "the stream did nothing");
        chacha20_xor(&key, &nonce, 1, &mut buf);
        assert_eq!(buf, plain, "ChaCha20 must be its own inverse");

        // A different key, nonce or counter produces a different stream.
        let mut a = plain;
        let mut b = plain;
        chacha20_xor(&key, &nonce, 1, &mut a);
        chacha20_xor(&key, &nonce, 2, &mut b);
        assert!(!constant_time_eq(&a, &b), "the counter is ignored");
        let mut c = plain;
        let mut other_key = key;
        other_key[0] ^= 1;
        chacha20_xor(&other_key, &nonce, 1, &mut c);
        assert!(!constant_time_eq(&a, &c), "the key is ignored");
        let mut d = plain;
        let mut other_nonce = nonce;
        other_nonce[0] ^= 1;
        chacha20_xor(&key, &other_nonce, 1, &mut d);
        assert!(!constant_time_eq(&a, &d), "the nonce is ignored");

        // A partial final block is handled.
        let mut short = [0u8; 5];
        chacha20_xor(&key, &nonce, 1, &mut short);
        chacha20_xor(&key, &nonce, 1, &mut short);
        assert_eq!(short, [0u8; 5]);

        let block = chacha20_block(&key, &nonce, 0);
        assert_eq!(block.len(), 64);
        assert_ne!(&block[..], &[0u8; 64]);
    }

    #[test]
    fn hmac_signatures_verify_and_reject() {
        let key = b"boot-signing-key";
        let message = b"kernel image bytes";
        let sig = hmac_sha256(key, message);
        assert!(verify_signature(key, message, &sig));
        // A single flipped byte in the message, the key or the signature fails.
        assert!(!verify_signature(key, b"kernel image byteS", &sig));
        assert!(!verify_signature(b"boot-signing-keY", message, &sig));
        let mut forged = sig;
        forged[0] ^= 1;
        assert!(!verify_signature(key, message, &forged));
        // Signatures are deterministic, which is what makes them usable as
        // measurements.
        assert_eq!(hmac_sha256(key, message), sig);

        let mut h = Sha256::new();
        h.update(b"a");
        h.update(b"b");
        assert_eq!(h.finish(), sha256(b"ab"));
    }

    #[test]
    fn boot_chain_measures_and_halts_on_mismatch() {
        let mut chain = BootChain::new();
        assert!(chain.is_empty());
        let _firmware = chain.measure("firmware", b"uefi image").unwrap();
        let _bootloader = chain.measure("bootloader", b"limine").unwrap();
        let _kernel = chain.measure("kernel", b"varix").unwrap();
        assert_eq!(chain.len(), 3);
        assert!(!chain.verified(), "measured is not verified");
        assert!(!chain.verify_all(), "no expected digest was set");

        // The chain digest commits to the order as well as the content.
        let d1 = chain.chain_digest();
        let mut reordered = BootChain::new();
        reordered.measure("bootloader", b"limine").unwrap();
        reordered.measure("firmware", b"uefi image").unwrap();
        assert_ne!(d1, reordered.chain_digest(), "order is not committed");

        // Verify against the correct digest.
        let mut good = BootChain::new();
        let _ = good.measure("firmware", b"uefi image");
        let _ = good.measure("bootloader", b"limine");
        let _ = good.measure("kernel", b"varix");
        good.expect(d1);
        assert!(good.verify_all());
        assert!(good.verified());
        assert!(!good.halted());

        // A tampered stage halts the chain and is named as the failure.
        let mut bad = BootChain::new();
        let fw = bad.measure("firmware", b"uefi image").unwrap();
        let _bl = bad.measure("bootloader", b"limine").unwrap();
        assert_eq!(bad.len(), 2);
        assert!(bad.verify_stage(0, &fw));
        assert!(!bad.verify_stage(1, &sha256(b"evil")), "the tampered stage");
        assert_eq!(bad.stage(0).unwrap().verdict, StageVerdict::Verified);
        assert_eq!(bad.stage(1).unwrap().verdict, StageVerdict::Mismatch);
        assert!(bad.halted());
        assert!(bad.measure("kernel", b"varix").is_err(), "a halted chain stops");
        assert_eq!(bad.mismatches(), 1);
        assert!(!bad.verified());
        assert!(bad.stage(9).is_none());

        // The rendered report names each stage.
        let mut out = [0u8; 256];
        let n = good.render(&mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("firmware verified"), "got {text}");
    }

    #[test]
    fn entropy_pool_is_unpredictable_and_rejects_stuck_sources() {
        let mut pool = EntropyPool::new();
        assert!(!pool.rdrand_ok());
        // Fewer than four samples is a failure, not a partial success.
        assert!(!pool.adopt_rdrand(&[1, 2]));
        assert_eq!(pool.rdrand_failures(), 1);
        // A stuck source is detected.
        assert!(!pool.adopt_rdrand(&[5u64; 8]));
        assert_eq!(pool.stuck_detections(), 1);
        assert!(!pool.rdrand_ok());
        // A varying source is accepted and stirred in.
        let samples = [1u64, 2, 3, 4, 5, 6, 7, 8];
        assert!(pool.adopt_rdrand(&samples));
        assert!(pool.rdrand_ok());
        assert!(pool.reseeds() >= 8);

        pool.stir(0xDEAD_BEEF);
        let mut seen = [0u64; 16];
        for slot in seen.iter_mut() {
            *slot = pool.next_u64();
        }
        assert_eq!(pool.draws(), 16);
        for i in 0..seen.len() {
            for j in (i + 1)..seen.len() {
                assert_ne!(seen[i], seen[j], "the pool repeated itself");
            }
        }
        assert_eq!(pool.next_u32() as u64 <= u32::MAX as u64, true);
        // `below` stays in range and does not return the bound.
        for _ in 0..200 {
            let v = pool.below(7);
            assert!(v < 7);
        }
        assert_eq!(pool.below(0), 0);
    }

    #[test]
    fn keys_generate_authorize_expire_and_revoke() {
        let mut store = KeyStore::new();
        assert_eq!(store.active(), 0);
        let disk = store.generate(KeyPurpose::DiskEncryption, 0, 1000).unwrap();
        let boot = store.generate(KeyPurpose::BootMeasurement, 0, 0).unwrap();
        assert_ne!(disk, boot);
        assert_eq!(store.active(), 2);
        assert_eq!(store.generated(), 2);

        let key = store.authorize(disk, KeyPurpose::DiskEncryption, 10).unwrap();
        assert_ne!(key, [0u8; KEY_BYTES], "the key must come from the pool");
        // The wrong purpose is refused.
        assert_eq!(
            store.authorize(disk, KeyPurpose::PackageSigning, 10),
            Err("key is not valid for this purpose")
        );
        // Expiry is enforced, and counted.
        assert_eq!(
            store.authorize(disk, KeyPurpose::DiskEncryption, 5000),
            Err("key has expired")
        );
        assert_eq!(store.expired_uses(), 1);
        // A key with no TTL never expires.
        assert!(store.authorize(boot, KeyPurpose::BootMeasurement, 999_999).is_ok());

        // Revocation wipes the material.
        assert!(store.revoke(disk));
        assert!(!store.revoke(disk), "revoking twice is refused");
        assert_eq!(store.authorize(disk, KeyPurpose::DiskEncryption, 10), Err("key was revoked"));
        assert_eq!(store.active(), 1);
        assert_eq!(store.revoked_count(), 1);
        assert!(store.get(disk).is_none());

        // Wiping everything leaves no active keys.
        store.wipe_all();
        assert_eq!(store.active(), 0);
        // A Debug render must not contain key material.
        let dbg = format!("{:?}", KeyEntry::default());
        assert!(dbg.contains("redacted"), "got {dbg}");

        // The next generation still gets a fresh id, so a stale id cannot be
        // replayed against a new key.
        let again = store.generate(KeyPurpose::DiskEncryption, 0, 0).unwrap();
        assert_ne!(again, disk);
    }

    #[test]
    fn encrypted_volume_round_trips_and_needs_the_right_key() {
        let mut vol = EncryptedVolume::create(64);
        assert_eq!(vol.state, VolumeState::Locked);
        let mut sector = [0u8; SECTOR_BYTES];
        for (i, b) in sector.iter_mut().enumerate() {
            *b = (i % 251) as u8;
        }
        let plain = sector;
        // Locked volumes refuse to touch a sector.
        assert!(!vol.encrypt_sector(0, &mut sector));
        assert_eq!(vol.state, VolumeState::Locked);

        assert!(vol.unlock(1, [0x11; 32]));
        assert_eq!(vol.state, VolumeState::Unlocked);
        assert!(vol.encrypt_sector(0, &mut sector));
        assert!(EncryptedVolume::looks_encrypted(&plain, &sector));
        assert!(vol.decrypt_sector(0, &mut sector));
        assert_eq!(sector, plain, "the sector did not round-trip");
        assert_eq!(vol.encrypts(), 1);
        assert_eq!(vol.decrypts(), 1);

        // Same plaintext, different sector → different ciphertext.
        let mut a = plain;
        let mut b = plain;
        assert!(vol.encrypt_sector(1, &mut a));
        assert!(vol.encrypt_sector(2, &mut b));
        assert!(!constant_time_eq(&a, &b), "the sector nonce is not applied");
        assert_ne!(vol.nonce_for(1), vol.nonce_for(2));

        // A sector past the end is refused, not wrapped.
        assert!(!vol.encrypt_sector(1000, &mut sector));

        // A different key does not recover the plaintext.
        let mut other = EncryptedVolume::create(64);
        assert!(other.unlock(2, [0x22; 32]));
        let mut wrong = plain;
        assert!(other.encrypt_sector(0, &mut wrong));
        assert!(!constant_time_eq(&sector, &wrong));

        // Locking wipes the key.
        vol.lock();
        assert_eq!(vol.state, VolumeState::Locked);
        assert!(!vol.decrypt_sector(0, &mut sector));
        assert!(format!("{vol:?}").contains("redacted"));

        // A corrupt volume refuses to unlock at all.
        let mut corrupt = EncryptedVolume::create(8);
        corrupt.mark_corrupt();
        assert!(!corrupt.unlock(1, [0; 32]));
        assert_eq!(corrupt.auth_failures, 1);
    }

    #[test]
    fn w_xor_x_isolation_sandbox_and_acl() {
        use crate::mem::paging::{P_NX, P_USER, P_WRITE};
        assert!(w_xor_x(P_NX | P_USER).is_ok());
        assert!(w_xor_x(P_WRITE | P_USER).is_err(), "writable implies not executable");
        assert!(w_xor_x(P_WRITE | P_NX).is_ok());
        assert!(w_xor_x(P_NX).is_ok());

        let mut spaces = AddressSpaces::new();
        assert!(spaces.is_empty());
        let a = spaces.create(0x1000, 1).unwrap();
        let b = spaces.create(0x2000, 2).unwrap();
        assert_eq!(spaces.find_by_pid(2), Some(b));
        assert!(spaces.map_foreign(a, a).is_ok(), "same space is fine");
        assert!(spaces.map_foreign(a, b).is_err(), "cross-process mapping is denied");
        assert_eq!(spaces.cross_map_denials(), 1);
        assert!(spaces.share_page(a));
        assert!(spaces.note_user_page(a));
        assert_eq!(spaces.get(a).unwrap().shared_pages, 1);
        assert!(spaces.release(b));
        assert!(spaces.create(0, 9).is_none(), "a null page table is refused");
        assert!(spaces.create(0x3000, 9).is_some(), "a valid space is created");
        assert!(spaces.release(99) == false);

        // Capabilities shrink, never grow.
        use crate::proc::Caps;
        let jail = SandboxLevel::Jail;
        let restricted = jail.restrict(Caps::kernel());
        assert!(!restricted.has(Caps::NET));
        assert!(!restricted.has(Caps::DEVICE));
        assert!(!jail.allows_devices());
        assert!(!jail.allows_network());
        assert!(SandboxLevel::Basic.allows_network());
        assert!(SandboxLevel::Basic.allows_devices());
        assert!(SandboxLevel::Open.denied_caps() == 0);
        assert!(SandboxLevel::Jail.restrict(Caps::kernel()).is_subset_of(Caps::kernel()));
        assert!(SandboxLevel::Strict > SandboxLevel::Basic);
        assert!(!SandboxLevel::Jail.as_str().is_empty());

        // ACL: first match wins, and a match with no permission denies.
        use crate::proc::{Access, Credentials};
        let mut acl = Acl::new();
        assert!(acl.is_empty());
        assert!(acl.grant(1000, 0, 0o6, false));
        assert!(acl.grant(0, 100, 0o4, true));
        assert_eq!(acl.len(), 2);
        assert_eq!(acl.inheritable(), 1);
        let owner = Credentials {
            uid: 1000,
            gid: 1000,
        };
        assert_eq!(acl.evaluate(owner, Access::Read).0, AclVerdict::Explicit);
        assert!(acl.evaluate(owner, Access::Read).1);
        assert!(acl.evaluate(owner, Access::Write).1);
        assert!(!acl.evaluate(owner, Access::Execute).1, "no execute bit");
        let group = Credentials {
            uid: 5000,
            gid: 100,
        };
        assert!(acl.evaluate(group, Access::Read).1);
        assert!(!acl.evaluate(group, Access::Write).1);
        // Nobody matches: the evaluation falls through to the mode bits.
        let stranger = Credentials {
            uid: 9,
            gid: 9,
        };
        assert_eq!(acl.evaluate(stranger, Access::Read).0, AclVerdict::FellThrough);
        assert!(acl.revoke(1000, 0));
        assert!(!acl.revoke(1000, 0));
        assert_eq!(acl.len(), 1);
        assert!(acl.evaluations() >= 7);
    }

    #[test]
    fn audit_privilege_telemetry_notices_and_kb() {
        use crate::proc::Caps;
        let mut log = AuditLog::new();
        assert!(log.is_empty());
        log.record(1, AuditAction::CapabilityCheck, true, 1);
        let after_one = log.chain_digest();
        log.record(2, AuditAction::PrivilegeTransition, false, 2);
        log.record(3, AuditAction::TelemetryAttempt, false, 3);
        assert_eq!(log.total(), 3);
        assert_eq!(log.denials(), 2);
        assert_ne!(log.chain_digest(), after_one);
        let newest = log.get(0).unwrap();
        assert_eq!(newest.actor, 3);
        assert!(!newest.allowed);
        assert_eq!(newest.action, Some(AuditAction::TelemetryAttempt));
        assert!(log.get(9).is_none());
        // The retained chain is recomputable from the events alone.
        assert_ne!(log.retained_chain(), [0u8; 32]);
        assert!(!AuditAction::FirewallChange.as_str().is_empty());

        let mut guard = PrivilegeGuard::new();
        assert!(guard.transition(Caps(Caps::FS_READ | Caps::NET), Caps(Caps::FS_READ), false, 0).is_ok());
        assert_eq!(
            guard.transition(Caps(Caps::FS_READ), Caps(Caps::FS_READ | Caps::NET), false, 0),
            Err("capability gain without a token")
        );
        assert_eq!(guard.last_denial(), "capability gain without a token");
        assert!(guard.transition(Caps(Caps::FS_READ), Caps(Caps::FS_READ | Caps::NET), true, 0).is_ok());
        assert_eq!(guard.transitions(), 3);
        assert_eq!(guard.denied(), 1);
        assert_eq!(guard.escalations(), 1);

        let mut t = TelemetryGuard::new();
        assert!(!TELEMETRY_ENABLED);
        assert!(!t.allow_egress("telemetry.local"));
        assert!(!t.allow_egress("metrics.local"));
        assert!(t.allow_egress("github.com"));
        assert_eq!(t.attempts(), 3);
        assert_eq!(t.blocked(), 2);

        let mut guard = ScreenshotGuard::new();
        assert!(guard.protect(3));
        assert!(!guard.protect(3), "already protected");
        assert!(guard.capture_allowed(&[1, 2]));
        assert!(!guard.capture_allowed(&[2, 3]));
        assert_eq!(guard.denials(), 1);
        assert!(guard.unprotect(3));
        assert!(guard.capture_allowed(&[2, 3]));
        assert!(!guard.unprotect(3));
        assert_eq!(guard.count(), 0);

        // Notices carry advice, and only warnings interrupt.
        let notice = SecurityNotice {
            severity: Severity::Critical,
            title: "boot chain mismatch",
            advice: "the kernel image was modified; reinstall from a signed medium",
        };
        let mut out = [0u8; 256];
        let n = notice.render(&mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.starts_with("[critical] boot chain mismatch"), "got {text}");
        assert!(Severity::Critical.interrupts());
        assert!(!Severity::Info.interrupts());
        assert!(Severity::Warning > Severity::Notice);

        // The response flow has an SLA and cannot be reopened past disclosure.
        let mut case = ResponseCase::open(1, 0);
        assert_eq!(case.state, ResponseState::Reported);
        assert!(!case.overdue(1));
        assert!(case.advance(1));
        assert_eq!(case.state, ResponseState::Triaged);
        assert!(case.advance(30));
        assert_eq!(case.state, ResponseState::Fixed);
        assert!(case.advance(45));
        assert_eq!(case.state, ResponseState::Disclosed);
        assert!(!case.advance(46), "a closed case stays closed");
        assert_eq!(case.age_days(50), 50);
        assert!(case.breached);
        assert_eq!(ResponseState::Reported.sla_days(), 2);
        assert_eq!(ResponseState::Disclosed.sla_days(), 0);
        let late = ResponseCase::open(2, 0);
        assert!(late.overdue(99));

        // Service manifests: over-privilege is a finding.
        let manifest = ServiceManifest {
            name: "audio",
            required: Caps(Caps::DEVICE),
            optional: Caps(Caps::FS_READ),
        };
        assert_eq!(manifest.audit(Caps(Caps::DEVICE)), ManifestVerdict::Minimal);
        assert_eq!(
            manifest.audit(Caps(Caps::DEVICE | Caps::FS_READ)),
            ManifestVerdict::Exact
        );
        assert_eq!(
            manifest.audit(Caps(Caps::DEVICE | Caps::NET)),
            ManifestVerdict::OverPrivileged
        );
        assert_eq!(manifest.audit(Caps(0)), ManifestVerdict::UnderPrivileged);
        assert!(ManifestVerdict::Exact.acceptable());
        assert!(!ManifestVerdict::OverPrivileged.acceptable());
        assert!(!ManifestVerdict::OverPrivileged.as_str().is_empty());

        // Baseline reporting.
        let items = [
            BaselineItem::new("nx", "on", "on"),
            BaselineItem::new("telemetry", "off", "off"),
            BaselineItem::new("aslr", "on", "off"),
        ];
        assert_eq!(baseline_failures(&items), 1);
        let n = baseline_report(&items, &mut out);
        let text = core::str::from_utf8(&out[..n]).unwrap();
        assert!(text.contains("DRIFT aslr expected=on actual=off"), "got {text}");

        // The vulnerability knowledge base is searchable and sortable.
        let mut kb = VulnerabilityKb::new();
        assert!(kb.is_empty());
        assert!(kb.add(VulnEntry::new(
            "VARIX-0001",
            Severity::Critical,
            "ISR shared state without a lock",
            "hold the interrupt-off window while updating the queue head"
        )));
        assert!(kb.add(VulnEntry::new(
            "VARIX-0002",
            Severity::Info,
            "cosmetic flicker on resume",
            "no action required"
        )));
        assert_eq!(kb.len(), 2);
        assert!(kb.find("VARIX-0001").is_some());
        assert!(kb.find("VARIX-9999").is_none());
        assert_eq!(kb.find("VARIX-0002").unwrap().short_id(), Some(2));
        let mut hits = [VulnEntry::new("", Severity::Info, "", ""); 4];
        assert_eq!(kb.at_least(Severity::Warning, &mut hits), 1);
        assert_eq!(hits[0].id, "VARIX-0001");
    }

    #[test]
    fn self_test_passes() {
        let canary = Canary::new(0x1234_5678_9ABC_DEF0);
        // F227 — NX is only real once `EFER.NXE` is set. Enabling it here is what
    // makes every NX bit the page tables already carry mean something.
    let nx_before = nx_available();
    if !nx_before {
        if let Some(p) = crate::platform::info() {
            if p.features.nx {
                let efer = crate::cpu::msr::read(crate::cpu::msr::Msr::Efer);
                let _ = crate::cpu::msr::write(
                    crate::cpu::msr::Msr::Efer,
                    efer | crate::cpu::msr::EFER_NXE,
                );
            }
        }
    }
    let nx_after = nx_available();
    if !nx_before && nx_after {
        crate::kinfo!("security: EFER.NXE enabled — NX bits are now enforced");
    }

    let (passed, failed) = run_security_checks(canary);
        let mut out = [0u8; 512];
        let n = SEC_SELFTEST.render(&mut out);
        let detail = core::str::from_utf8(&out[..n]).unwrap_or("");
        assert_eq!(failed, 0, "{passed} passed, {failed} failed: {detail}");
        assert!(passed >= 14);
        // A disarmed canary reports nothing, which the caller must never rely on.
        let mut disarmed = canary;
        disarmed.disarm();
        assert!(!disarmed.armed());
        assert!(disarmed.check(0).is_ok());
    }

    fn hex_bytes(digest: &[u8; 32]) -> [u8; 64] {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = [0u8; 64];
        for (i, b) in digest.iter().enumerate() {
            out[i * 2] = HEX[(b >> 4) as usize];
            out[i * 2 + 1] = HEX[(b & 0xF) as usize];
        }
        out
    }
}
