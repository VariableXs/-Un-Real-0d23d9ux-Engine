//! F019 平台探测器 — CPUID full feature scan.
//! F020 异构核探测 — hybrid (P/E core) topology recognition.
//!
//! Pure register-value decoding (host-testable) + a thin target `detect()`
//! that issues the actual `cpuid` instructions. The topology result feeds
//! AI-04's scheduler core-class hints; per-core classification happens at
//! SMP bring-up (AI-02) using the primitive defined here.

// ---------------------------------------------------------------------------
// F019 — CPUID feature scan
// ---------------------------------------------------------------------------

/// CPU vendor from CPUID leaf 0.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vendor {
    Intel,
    Amd,
    Unknown([u8; 12]),
}

impl Vendor {
    pub fn from_leaf0(ebx: u32, ecx: u32, edx: u32) -> Vendor {
        let mut name = [0u8; 12];
        name[0..4].copy_from_slice(&ebx.to_le_bytes());
        name[4..8].copy_from_slice(&edx.to_le_bytes());
        name[8..12].copy_from_slice(&ecx.to_le_bytes());
        match &name {
            b"GenuineIntel" => Vendor::Intel,
            b"AuthenticAMD" => Vendor::Amd,
            _ => Vendor::Unknown(name),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Vendor::Intel => "Intel",
            Vendor::Amd => "AMD",
            Vendor::Unknown(_) => "unknown",
        }
    }
}

/// Signature decode of CPUID leaf 1 EAX.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature {
    pub stepping: u8,
    pub model: u8,
    pub family: u8,
    /// Extended model bits (IA-32 SDM Vol.2 Eq. 3-1) merged into `model`.
    pub extended_model: bool,
}

impl Signature {
    pub fn from_eax(eax: u32) -> Signature {
        let stepping = (eax & 0xF) as u8;
        let model = ((eax >> 4) & 0xF) as u8;
        let family = ((eax >> 8) & 0xF) as u8;
        let ext_model = ((eax >> 16) & 0xF) as u8;
        let ext_family = ((eax >> 20) & 0xFF) as u8;
        let (family, model, extended) = if family == 0x0F {
            (family + ext_family, model + (ext_model << 4), true)
        } else {
            (family, model + (ext_model << 4), ext_model != 0)
        };
        Signature {
            stepping,
            model,
            family,
            extended_model: extended,
        }
    }
}

/// The feature set the boot phase cares about (F019 result).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Features {
    // leaf 1 (EDX)
    pub fpu: bool,
    pub sse: bool,
    pub sse2: bool,
    // leaf 1 (ECX)
    pub sse3: bool,
    pub ssse3: bool,
    pub sse4_1: bool,
    pub sse4_2: bool,
    pub popcnt: bool,
    pub aes: bool,
    pub pclmulqdq: bool,
    pub rdrand: bool,
    pub apic: bool,
    pub x2apic: bool,
    pub tsc_deadline: bool,
    pub hypervisor: bool,
    // leaf 7 subleaf 0
    pub avx2: bool,
    pub bmi1: bool,
    pub bmi2: bool,
    pub smap: bool,
    pub smep: bool,
    pub fsgsbase: bool,
    // leaf 7 subleaf 0 (EDX)
    pub hybrid: bool,
    // leaf 0x80000001
    pub nx: bool,
    pub rdtscp: bool,
    pub la57: bool,
    pub gb_pages: bool,
}

impl Features {
    /// Decode from the raw CPUID register values.
    ///
    /// * `l1c`/`l1d` — leaf 1 ECX/EDX
    /// * `l7b`/`l7d` — leaf 7 subleaf 0 EBX/EDX
    /// * `e1d` — extended leaf 0x80000001 EDX
    pub fn from_raw(l1c: u32, l1d: u32, l7b: u32, l7d: u32, e1d: u32, e1c: u32) -> Features {
        Features {
            fpu: l1d & (1 << 0) != 0,
            sse: l1d & (1 << 25) != 0,
            sse2: l1d & (1 << 26) != 0,
            sse3: l1c & (1 << 0) != 0,
            ssse3: l1c & (1 << 9) != 0,
            sse4_1: l1c & (1 << 19) != 0,
            sse4_2: l1c & (1 << 20) != 0,
            popcnt: l1c & (1 << 23) != 0,
            aes: l1c & (1 << 25) != 0,
            pclmulqdq: l1c & (1 << 1) != 0,
            rdrand: l1c & (1 << 30) != 0,
            apic: l1d & (1 << 9) != 0,
            x2apic: l1c & (1 << 21) != 0,
            tsc_deadline: l1c & (1 << 24) != 0,
            hypervisor: l1c & (1 << 31) != 0,
            avx2: l7b & (1 << 5) != 0,
            bmi1: l7b & (1 << 3) != 0,
            bmi2: l7b & (1 << 8) != 0,
            smap: l7b & (1 << 20) != 0,
            smep: l7b & (1 << 7) != 0,
            fsgsbase: l7b & (1 << 0) != 0,
            hybrid: l7d & (1 << 15) != 0,
            nx: e1d & (1 << 20) != 0,
            rdtscp: e1d & (1 << 27) != 0,
            la57: e1c & (1 << 16) != 0,
            gb_pages: e1d & (1 << 29) != 0,
        }
    }

    /// The single-line report for the boot log (F019).
    pub fn summary_bits(&self) -> u32 {
        let mut v = 0u32;
        let mut i = 0;
        for present in [
            self.sse2, self.sse4_2, self.avx2, self.bmi2, self.aes,
            self.rdrand, self.nx, self.gb_pages, self.smap, self.la57,
            self.x2apic, self.hybrid,
        ] {
            if present {
                v |= 1 << i;
            }
            i += 1;
        }
        v
    }
}

// ---------------------------------------------------------------------------
// F020 — hybrid topology
// ---------------------------------------------------------------------------

/// CPUID leaf 0x1A core type (EAX bits 31:24).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoreType {
    /// 0x40 — Intel Core performance core.
    Performance,
    /// 0x20 — Intel Atom efficiency core.
    Efficiency,
    Unknown(u8),
}

impl CoreType {
    pub fn from_leaf1a_eax(eax: u32) -> CoreType {
        let native_id = (eax >> 24) & 0xFF;
        match native_id {
            0x40 => CoreType::Performance,
            0x20 => CoreType::Efficiency,
            other => CoreType::Unknown(other as u8),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CoreType::Performance => "P",
            CoreType::Efficiency => "E",
            CoreType::Unknown(_) => "?",
        }
    }
}

/// Machine-level topology class (what boot can know pre-SMP).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topology {
    /// Uniform cores, no hybrid flag.
    Uniform {
        cores: u32,
    },
    /// Hybrid CPU detected (leaf 7 EDX bit 15); per-core P/E split is filled
    /// in by AI-02 at SMP bring-up using [`CoreType::from_leaf1a_eax`].
    Hybrid {
        cores: u32,
        native_id_support: bool,
    },
    /// CPUID unavailable / not yet scanned.
    Unknown,
}

impl Topology {
    pub fn cores(&self) -> Option<u32> {
        match self {
            Topology::Uniform { cores } | Topology::Hybrid { cores, .. } => Some(*cores),
            Topology::Unknown => None,
        }
    }

    pub fn is_hybrid(&self) -> bool {
        matches!(self, Topology::Hybrid { .. })
    }
}

/// Derive the topology from already-collected CPUID data.
/// `initial_lapic_id` is CPUID leaf 1 EBX bits 31:24, used to sanity check
/// APIC id width for core counting (physical core count ultimately comes
/// from the MADT — this is the cross-check).
pub fn topology_from(hybrid_flag: bool, leaf1a_supported: bool) -> Topology {
    if hybrid_flag {
        Topology::Hybrid {
            cores: 0, // filled by MADT cross-check at boot
            native_id_support: leaf1a_supported,
        }
    } else {
        Topology::Uniform { cores: 0 }
    }
}

// ---------------------------------------------------------------------------
// TSC frequency (leaf 0x15 / 0x16) — used by the boot timeline (F024)
// ---------------------------------------------------------------------------

/// Parse CPUID leaf 0x15 (nominal core crystal clock).
/// Returns ticks per second when both numerator and denominator are valid.
pub fn tsc_hz_from_leaf15(eax: u32, ebx: u32, ecx: u32) -> Option<u64> {
    let denom = eax as u64;
    let numer = ebx as u64;
    if denom == 0 || numer == 0 {
        return None;
    }
    let hz = ecx as u64;
    if hz == 0 {
        return None; // nominal crystal not enumerated; fall back to leaf 0x16
    }
    Some(hz * numer / denom)
}

/// Parse CPUID leaf 0x16 (base frequency in MHz).
pub fn tsc_hz_from_leaf16(eax: u32) -> Option<u64> {
    let mhz = eax as u64;
    if mhz == 0 {
        None
    } else {
        Some(mhz * 1_000_000)
    }
}

/// Fallback TSC frequency used when the CPU does not enumerate it
/// (QEMU TCG default-ish 2.5 GHz).
pub const FALLBACK_TSC_HZ: u64 = 2_500_000_000;

// ---------------------------------------------------------------------------
// Target detection (cpuid instructions)
// ---------------------------------------------------------------------------

use crate::once::OnceLock;

/// Complete platform probe result.
#[derive(Clone, Copy, Debug)]
pub struct PlatformInfo {
    pub vendor: Vendor,
    pub signature: Signature,
    pub features: Features,
    pub topology: Topology,
    pub brand: &'static str,
    pub tsc_hz: u64,
}

static INFO: OnceLock<PlatformInfo> = OnceLock::new();

pub fn info() -> Option<&'static PlatformInfo> {
    INFO.get()
}

/// CPU brand string buffer (leaf 0x80000002..04, 48 bytes).
/// Atomics keep the buffer writable without `static mut`.
#[cfg(target_os = "none")]
const ZERO_U8: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);
#[cfg(target_os = "none")]
static BRAND: [core::sync::atomic::AtomicU8; 49] = [ZERO_U8; 49];

#[cfg(target_os = "none")]
pub fn detect() -> PlatformInfo {
    use core::arch::x86_64::__cpuid_count;
    {
        // Leaf 0: vendor + max leaf.
        let l0 = __cpuid_count(0, 0);
        let vendor = Vendor::from_leaf0(l0.ebx, l0.ecx, l0.edx);
        let max_basic = l0.eax;

        let (l1c, l1d, sig_eax) = if max_basic >= 1 {
            let l1 = __cpuid_count(1, 0);
            (l1.ecx, l1.edx, l1.eax)
        } else {
            (0, 0, 0)
        };

        let (l7b, l7d, leaf1a_supported) = if max_basic >= 7 {
            let l7 = __cpuid_count(7, 0);
            (l7.ebx, l7.edx, max_basic >= 0x1A)
        } else {
            (0, 0, false)
        };

        // Extended leaves.
        let le = __cpuid_count(0x8000_0000, 0);
        let max_ext = le.eax;
        let (e1d, e1c) = if max_ext >= 1 {
            let e1 = __cpuid_count(0x8000_0001, 0);
            (e1.edx, e1.ecx)
        } else {
            (0, 0)
        };

        // Brand string.
        let mut brand = "unknown";
        if max_ext >= 4 {
            let l2 = __cpuid_count(0x8000_0002, 0);
            let l3 = __cpuid_count(0x8000_0003, 0);
            let l4 = __cpuid_count(0x8000_0004, 0);
            let words = [l2, l3, l4];
            let mut n = 0usize;
            for w in words.iter() {
                for reg in [w.eax, w.ebx, w.ecx, w.edx] {
                    for b in reg.to_le_bytes() {
                        if n < 48 {
                            BRAND[n].store(b, core::sync::atomic::Ordering::Relaxed);
                            n += 1;
                        }
                    }
                }
            }
            BRAND[48].store(0, core::sync::atomic::Ordering::Relaxed);
            // SAFETY: `BRAND` is a `static`, so the byte view (and every
            // subslice of it) is `'static`; it is only ever written here on
            // the single-threaded boot path and `AtomicU8` has `u8` layout.
            let raw: &'static [u8] = unsafe {
                core::slice::from_raw_parts(BRAND.as_ptr().cast::<u8>(), 48)
            };
            // Trim leading spaces.
            let start = raw.iter().position(|&b| b != b' ').unwrap_or(48);
            let end = raw.iter().rposition(|&b| b != 0 && b != b' ').map(|p| p + 1).unwrap_or(start);
            if start < end {
                if let Ok(s) = core::str::from_utf8(&raw[start..end]) {
                    brand = s;
                }
            }
        }

        // TSC frequency.
        let mut tsc_hz = None;
        if max_basic >= 0x16 {
            let l16 = __cpuid_count(0x16, 0);
            tsc_hz = tsc_hz_from_leaf16(l16.eax);
        }
        if tsc_hz.is_none() && max_basic >= 0x15 {
            let l15 = __cpuid_count(0x15, 0);
            tsc_hz = tsc_hz_from_leaf15(l15.eax, l15.ebx, l15.ecx);
        }

        let features = Features::from_raw(l1c, l1d, l7b, l7d, e1d, e1c);
        let topology = topology_from(features.hybrid, leaf1a_supported);
        let info = PlatformInfo {
            vendor,
            signature: Signature::from_eax(sig_eax),
            features,
            topology,
            brand,
            tsc_hz: tsc_hz.unwrap_or(FALLBACK_TSC_HZ),
        };
        let _ = INFO.set(info);
        info
    }
}

#[cfg(not(target_os = "none"))]
pub fn detect() -> PlatformInfo {
    // Host build: no cpuid here — return a synthetic QEMU-like profile so
    // the boot sequence source stays identical under cfg.
    let info = PlatformInfo {
        vendor: Vendor::Intel,
        signature: Signature::from_eax(0),
        features: Features::default(),
        topology: Topology::Unknown,
        brand: "host-build",
        tsc_hz: FALLBACK_TSC_HZ,
    };
    let _ = INFO.set(info);
    info
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_decode() {
        // CPUID leaf 0 vendor string order is EBX:EDX:ECX ("Genu" "ineI" "ntel").
        let intel = Vendor::from_leaf0(
            u32::from_le_bytes(*b"Genu"), // EBX
            u32::from_le_bytes(*b"ntel"), // ECX
            u32::from_le_bytes(*b"ineI"), // EDX
        );
        assert_eq!(intel, Vendor::Intel);
        let amd = Vendor::from_leaf0(
            u32::from_le_bytes(*b"Auth"), // EBX
            u32::from_le_bytes(*b"cAMD"),  // ECX
            u32::from_le_bytes(*b"enti"),  // EDX
        );
        assert_eq!(amd, Vendor::Amd);
        let other = Vendor::from_leaf0(1, 2, 3);
        assert_eq!(other.as_str(), "unknown");
    }

    #[test]
    fn signature_decode_simple_and_extended() {
        // Family 6, model 0x9E (Coffee Lake refresh): EAX = 0x000906EA
        let s = Signature::from_eax(0x0009_06EA);
        assert_eq!(s.family, 6);
        assert_eq!(s.model, 0x9E);
        assert_eq!(s.stepping, 0xA);
        // Family 0xF + extended family 1 → family 0x10 (AMD Zen style)
        let s2 = Signature::from_eax(0x0010_0F82);
        assert_eq!(s2.family, 0x10);
        assert_eq!(s2.model, 0x08 + (0 << 4));
        assert_eq!(s2.stepping, 2);
        // Extended model: family 6 ext model 1 model 6 → 0x16
        let s3 = Signature::from_eax(0x0001_0660);
        assert_eq!(s3.family, 6);
        assert_eq!(s3.model, 0x16);
    }

    #[test]
    fn feature_decode_known_bits() {
        // Craft register values with exactly one bit set per feature.
        let f = Features::from_raw(
            (1 << 19) | (1 << 20), // sse4_1, sse4_2
            (1 << 25) | (1 << 26) | (1 << 9), // sse, sse2, apic
            (1 << 5) | (1 << 8), // avx2, bmi2
            1 << 15, // hybrid
            (1 << 20) | (1 << 29), // nx, gb_pages
            1 << 16, // la57
        );
        assert!(f.sse4_1 && f.sse4_2 && f.sse && f.sse2 && f.apic);
        assert!(f.avx2 && f.bmi2 && f.hybrid && f.nx && f.gb_pages && f.la57);
        assert!(!f.sse3);
        assert!(!f.aes);
        assert!(!f.hypervisor);
        assert!(!f.fpu);
    }

    #[test]
    fn feature_summary_roundtrip() {
        let mut f = Features::default();
        f.sse2 = true;
        f.nx = true;
        f.hybrid = true;
        let bits = f.summary_bits();
        assert_eq!(bits & 1, 1); // sse2 → bit 0
        assert_eq!(bits & (1 << 6), 1 << 6); // nx → bit 6
        assert_eq!(bits & (1 << 11), 1 << 11); // hybrid → bit 11
        assert_eq!(bits & (1 << 1), 0); // avx2 unset
    }

    #[test]
    fn core_type_decode() {
        assert_eq!(CoreType::from_leaf1a_eax(0x4000_0000), CoreType::Performance);
        assert_eq!(CoreType::from_leaf1a_eax(0x2000_0000), CoreType::Efficiency);
        assert_eq!(CoreType::from_leaf1a_eax(0x0100_0000), CoreType::Unknown(0x01));
        assert_eq!(CoreType::Performance.as_str(), "P");
        assert_eq!(CoreType::Efficiency.as_str(), "E");
        assert_eq!(CoreType::Unknown(9).as_str(), "?");
        // Model bits are in the low nibble and must not affect the type.
        assert_eq!(CoreType::from_leaf1a_eax(0x4000_001F), CoreType::Performance);
    }

    #[test]
    fn topology_classification() {
        let t = topology_from(false, false);
        assert_eq!(t, Topology::Uniform { cores: 0 });
        assert!(!t.is_hybrid());
        let t2 = topology_from(true, true);
        assert_eq!(
            t2,
            Topology::Hybrid { cores: 0, native_id_support: true }
        );
        assert!(t2.is_hybrid());
        assert_eq!(t2.cores(), Some(0));
        assert_eq!(Topology::Unknown.cores(), None);
    }

    #[test]
    fn tsc_frequency_decoders() {
        // 24 MHz crystal, 24/24 ratio → 24 MHz.
        assert_eq!(tsc_hz_from_leaf15(24, 24, 24_000_000), Some(24_000_000));
        // 38.4 MHz crystal, 96/38.4 → 96 MHz.
        assert_eq!(tsc_hz_from_leaf15(384, 960, 38_400_000), Some(96_000_000));
        // Invalid combinations.
        assert_eq!(tsc_hz_from_leaf15(0, 24, 24_000_000), None);
        assert_eq!(tsc_hz_from_leaf15(24, 0, 24_000_000), None);
        assert_eq!(tsc_hz_from_leaf15(24, 24, 0), None);
        // Leaf 0x16: 3100 MHz.
        assert_eq!(tsc_hz_from_leaf16(3100), Some(3_100_000_000));
        assert_eq!(tsc_hz_from_leaf16(0), None);
    }

    #[test]
    fn detect_on_host_returns_synthetic() {
        let p = detect();
        assert_eq!(p.brand, "host-build");
        assert_eq!(p.tsc_hz, FALLBACK_TSC_HZ);
        assert!(info().is_some());
    }
}
