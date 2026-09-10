//! F021 引导链完整性校验 — boot chain integrity: kernel image hashing +
//! expected-hash comparison (`khash=<hex>` cmdline knob) and bootloader
//! identity recording.
//!
//! Includes a compact SHA-256 (FIPS 180-4) so the check needs no external
//! crates. Policy (W1): a mismatch is a loud `kerror!` + self-test FAIL but
//! does not halt the machine — the emergency console stays usable; hard
//! enforcement arrives with the secure boot domain (AI-10).

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4)
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Incremental SHA-256 state.
#[derive(Clone)]
pub struct Sha256 {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Sha256::new()
    }
}

impl Sha256 {
    pub fn new() -> Sha256 {
        Sha256 {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
                0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        // Fill partial buffer first.
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            let take = need.min(data.len());
            self.buf[self.buf_len..self.buf_len + take].copy_from_slice(&data[..take]);
            self.buf_len += take;
            data = &data[take..];
            if self.buf_len == 64 {
                let block = self.buf;
                self.compress(&block);
                self.buf_len = 0;
            }
        }
        // Whole blocks directly.
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            self.compress(&block);
            data = &data[64..];
        }
        // Remainder.
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bit_len = self.total.wrapping_mul(8);
        // Padding: 0x80, zeros, 8-byte big-endian bit length.
        self.update(&[0x80]);
        while self.buf_len != 56 {
            self.update(&[0]);
        }
        // Manual length append (update() would corrupt total).
        let block_tail = bit_len.to_be_bytes();
        self.buf[56..64].copy_from_slice(&block_tail);
        let block = self.buf;
        self.compress(&block);
        let mut out = [0u8; 32];
        for (i, w) in self.state.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
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
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
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
}

/// One-shot SHA-256.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(data);
    h.finish()
}

/// Lowercase hex encoding of a 32-byte digest into 64 ASCII bytes.
pub fn hex32(digest: &[u8; 32]) -> [u8; 64] {
    let mut out = [0u8; 64];
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for i in 0..32 {
        out[i * 2] = HEX[(digest[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(digest[i] & 0xF) as usize];
    }
    out
}

/// Parse a 64-char hex string into a 32-byte digest.
pub fn unhex64(s: &str) -> Option<[u8; 32]> {
    let b = s.as_bytes();
    if b.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_val(b[i * 2])?;
        let lo = hex_val(b[i * 2 + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// F021 — boot chain verification
// ---------------------------------------------------------------------------

/// Verification verdict.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Image hashed, expected hash matched.
    Verified,
    /// Expected hash present but mismatched.
    Mismatch,
    /// No expected hash given (dev boot) — chain unverified.
    NoExpectedHash,
    /// Image bytes unavailable from the bootloader.
    NoImage,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Verified => "verified",
            Verdict::Mismatch => "MISMATCH",
            Verdict::NoExpectedHash => "unverified",
            Verdict::NoImage => "no-image",
        }
    }
}

/// Result of the boot-chain integrity check.
#[derive(Clone, Copy, Debug)]
pub struct ChainReport {
    pub verdict: Verdict,
    /// Hex digest prefix (8 chars) for the log line.
    pub digest_prefix: [u8; 8],
    pub image_size: u64,
}

/// Verify a kernel image against an expected digest.
pub fn verify(image: &[u8], expected: Option<&[u8; 32]>) -> ChainReport {
    let digest = sha256(image);
    let full = hex32(&digest);
    let verdict = match expected {
        None => Verdict::NoExpectedHash,
        Some(exp) if *exp == digest => Verdict::Verified,
        Some(_) => Verdict::Mismatch,
    };
    ChainReport {
        verdict,
        digest_prefix: [
            full[0], full[1], full[2], full[3],
            full[4], full[5], full[6], full[7],
        ],
        image_size: image.len() as u64,
    }
}

use crate::once::OnceLock;

static REPORT: OnceLock<ChainReport> = OnceLock::new();

/// Run the target-side verification: hash the Limine-loaded kernel image and
/// compare against the `khash=` cmdline knob (F021).
pub fn init() -> ChainReport {
    let expected = crate::cmdline::get("khash").and_then(unhex64);
    let report = match crate::limine::executable_file() {
        Some(file) if !file.address.is_null() && file.size > 0 => {
            let image =
                unsafe { core::slice::from_raw_parts(file.address, file.size as usize) };
            verify(image, expected.as_ref())
        }
        _ => ChainReport {
            verdict: Verdict::NoImage,
            digest_prefix: *b"--------",
            image_size: 0,
        },
    };
    let _ = REPORT.set(report);
    report
}

pub fn report() -> Option<&'static ChainReport> {
    REPORT.get()
}

// ---------------------------------------------------------------------------
// Tests — FIPS vectors + policy
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_str(d: &[u8; 32]) -> String {
        let h = hex32(d);
        String::from_utf8_lossy(&h).into_owned()
    }

    #[test]
    fn sha256_fips_vectors() {
        // "abc"
        assert_eq!(
            hex_str(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // empty string
        assert_eq!(
            hex_str(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // 64 bytes ("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq") — two-block boundary
        assert_eq!(
            hex_str(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // long input across many blocks
        let long = [b'a'; 1_000];
        assert_eq!(
            hex_str(&sha256(&long)),
            "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"
        );
    }

    #[test]
    fn incremental_matches_oneshot() {
        let data: Vec<u8> = (0u8..=255).cycle().take(700).collect();
        let mut h = Sha256::new();
        // Feed in awkward chunk sizes to exercise buffering.
        let mut off = 0;
        let chunks = [1usize, 63, 64, 65, 3, 200, 500];
        for c in chunks {
            let end = (off + c).min(data.len());
            h.update(&data[off..end]);
            off = end;
            if off >= data.len() {
                break;
            }
        }
        if off < data.len() {
            h.update(&data[off..]);
        }
        assert_eq!(h.finish(), sha256(&data));
    }

    #[test]
    fn hex_roundtrip() {
        let d = sha256(b"varix");
        let h = hex32(&d);
        assert_eq!(h.len(), 64);
        let parsed = unhex64(core::str::from_utf8(&h).unwrap()).unwrap();
        assert_eq!(parsed, d);
        // uppercase accepted
        let upper: String = hex_str(&d).to_uppercase();
        assert_eq!(unhex64(&upper).unwrap(), d);
        // bad inputs rejected
        assert!(unhex64("zz").is_none());
        assert!(unhex64("").is_none());
        assert!(unhex64(&hex_str(&d)[..63]).is_none());
    }

    #[test]
    fn verify_verdicts() {
        let image = b"kernel image bytes";
        let digest = sha256(image);
        // no expected → unverified
        let r = verify(image, None);
        assert_eq!(r.verdict, Verdict::NoExpectedHash);
        assert_eq!(r.image_size, image.len() as u64);
        // matching expected
        let r2 = verify(image, Some(&digest));
        assert_eq!(r2.verdict, Verdict::Verified);
        // mismatching expected
        let mut bad = digest;
        bad[0] ^= 1;
        let r3 = verify(image, Some(&bad));
        assert_eq!(r3.verdict, Verdict::Mismatch);
        // digest prefix matches the real digest's first 8 hex chars
        assert_eq!(&r2.digest_prefix, &hex32(&digest)[..8]);
    }
}
