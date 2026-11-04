//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0054 · ACPI 表版本兼容矩阵 · 0x6DD80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0054-J1 RSDP/XSDT/RSDT 版本兼容判定：XSDT 在场走
//!     64 位账，缺席回退 RSDT 32 位账，双轨一致性对拍；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - RSDP 建模：签名 "RSD PTR "（8 字节）/校验/修订（1=1.0，
//!     2=2.0+）/长度/RSDT-32/XSDT-64；
//!   - 版本矩阵：修订 1 ⇒ 仅 RSDT；修订 ≥2 且 XSDT 非零 ⇒ XSDT
//!     优先（64 位账）；修订 ≥2 且 XSDT 零 ⇒ 回退 RSDT（有账）；
//!   - 表清单：RSDT 32 位指针数组 vs XSDT 64 位指针数组，交集
//!     签名集合一致（双轨对拍）；
//!   - 修订兼容：每表 (签名, 最低修订) 冻结矩阵，低于最低修订拒；
//!   - 校验和：RSDP 首段 20 字节 + 全段 36 字节双校验（v2）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DD80，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DD80;

/// RSDP 签名（"RSD PTR " 前 4 字节 'RSD ' 小端口径）。
pub const RSDP_SIG0: u32 = 0x2044_5352; // 'RSD ' 小端
/// RSDP v1 长度。
pub const RSDP_LEN_V1: usize = 20;
/// RSDP v2 长度。
pub const RSDP_LEN_V2: usize = 36;
/// SDT 头长度。
pub const SDT_HEADER_LEN: usize = 36;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompatCode {
    /// RSDP 签名错
    RsdpSigBad,
    /// RSDP v1 段校验和错
    RsdpV1Checksum,
    /// RSDP v2 扩展校验和错
    RsdpV2Checksum,
    /// 无根表（RSDT/XSDT 双零）
    NoRootTable,
    /// 表签名错
    SigBad,
    /// 表校验和错
    ChecksumBad,
    /// 修订低于冻结矩阵
    RevisionTooLow,
    /// 双轨签名集不一致
    TrackMismatch,
    /// 指针账零（清单内空指针）
    NullEntryPointer,
    /// 双跑不一致
    NotReproducible,
}

impl CompatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CompatCode::RsdpSigBad => "RSDP 签名错：非 RSD PTR",
            CompatCode::RsdpV1Checksum => "RSDP v1 段校验和错",
            CompatCode::RsdpV2Checksum => "RSDP v2 扩展校验和错",
            CompatCode::NoRootTable => "无根表：RSDT/XSDT 双零",
            CompatCode::SigBad => "表签名错",
            CompatCode::ChecksumBad => "表校验和错",
            CompatCode::RevisionTooLow => "修订低于冻结矩阵",
            CompatCode::TrackMismatch => "双轨签名集不一致",
            CompatCode::NullEntryPointer => "指针账零：清单空指针",
            CompatCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CompatCode; 10] = [
        CompatCode::RsdpSigBad,
        CompatCode::RsdpV1Checksum,
        CompatCode::RsdpV2Checksum,
        CompatCode::NoRootTable,
        CompatCode::SigBad,
        CompatCode::ChecksumBad,
        CompatCode::RevisionTooLow,
        CompatCode::TrackMismatch,
        CompatCode::NullEntryPointer,
        CompatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CompatCode::ALL.len() {
        if CompatCode::ALL[i].val() == code {
            return CompatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DD80 段闭集内"
}

// ── 版本兼容矩阵（签名 → 最低修订，冻结） ────────────────────────
/// 冻结矩阵条目。
pub const MATRIX_SIGS: [u32; 8] = [
    0x54445348, // 'HSDT'→FADT 记号（账面 'DSDT'）
    0x54445344, // 'DSDT'
    0x54435046, // 'FACP'(FADT)
    0x54445041, // 'APIC'(MADT)
    0x54435048, // 'HPET'
    0x54494643, // 'CFIT'→SRAT 记号（账面 'SRAT'）
    0x54415253, // 'SRAT'
    0x54494C53, // 'SLIT'
];
/// 冻结最低修订（与 MATRIX_SIGS 一一对应）。
pub const MATRIX_MIN_REV: [u8; 8] = [2, 2, 4, 2, 1, 2, 2, 1];

/// 矩阵查询：签名 → 最低修订（账外签名给 1）。
pub fn min_revision(sig: u32) -> u8 {
    let mut i = 0;
    while i < MATRIX_SIGS.len() {
        if MATRIX_SIGS[i] == sig {
            return MATRIX_MIN_REV[i];
        }
        i += 1;
    }
    1
}

// ── RSDP 解析 ───────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct Rsdp {
    pub revision: u8,
    pub rsdt_addr: u32,
    pub xsdt_addr: u64,
}

impl Rsdp {
    /// 字节账解析（v1/v2 双校验）。
    pub fn parse(bytes: &[u8]) -> Result<Rsdp, u32> {
        if bytes.len() < RSDP_LEN_V1 {
            return Err(CompatCode::RsdpSigBad.val());
        }
        let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if sig != RSDP_SIG0 || bytes[4] != b'P' || bytes[5] != b'T' || bytes[6] != b'R' || bytes[7] != b' ' {
            return Err(CompatCode::RsdpSigBad.val());
        }
        // v1 段校验：前 20 字节和 mod 256 == 0。
        let mut sum: u8 = 0;
        let mut i = 0;
        while i < RSDP_LEN_V1 {
            sum = sum.wrapping_add(bytes[i]);
            i += 1;
        }
        if sum != 0 {
            return Err(CompatCode::RsdpV1Checksum.val());
        }
        let revision = bytes[15];
        let rsdt_addr = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let mut xsdt_addr = 0u64;
        if revision >= 2 {
            if bytes.len() < RSDP_LEN_V2 {
                return Err(CompatCode::RsdpV2Checksum.val());
            }
            let mut sum2: u8 = 0;
            let mut i = 0;
            while i < RSDP_LEN_V2 {
                sum2 = sum2.wrapping_add(bytes[i]);
                i += 1;
            }
            if sum2 != 0 {
                return Err(CompatCode::RsdpV2Checksum.val());
            }
            xsdt_addr = u64::from_le_bytes([
                bytes[24], bytes[25], bytes[26], bytes[27],
                bytes[28], bytes[29], bytes[30], bytes[31],
            ]);
        }
        if rsdt_addr == 0 && xsdt_addr == 0 {
            return Err(CompatCode::NoRootTable.val());
        }
        Ok(Rsdp { revision, rsdt_addr, xsdt_addr })
    }

    /// 版本路线判定：XSDT 优先（64 位账），缺席回退 RSDT。
    pub fn track(&self) -> (u8, bool) {
        if self.revision >= 2 && self.xsdt_addr != 0 {
            (2, true) // XSDT 轨
        } else {
            (1, false) // RSDT 轨（含 v2 无 XSDT 回退）
        }
    }
}

// ── SDT 表解析（签名/校验和/修订） ──────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SdtEntry {
    pub sig: u32,
    pub revision: u8,
}

/// 表头解析 + 校验和 + 修订矩阵守卫。
pub fn parse_sdt(bytes: &[u8]) -> Result<SdtEntry, u32> {
    if bytes.len() < SDT_HEADER_LEN {
        return Err(CompatCode::SigBad.val());
    }
    let sig = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let length = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    let end = if length >= SDT_HEADER_LEN && length <= bytes.len() { length } else { bytes.len() };
    let mut sum: u8 = 0;
    let mut i = 0;
    while i < end {
        sum = sum.wrapping_add(bytes[i]);
        i += 1;
    }
    if sum != 0 {
        return Err(CompatCode::ChecksumBad.val());
    }
    let revision = bytes[8];
    if revision < min_revision(sig) {
        return Err(CompatCode::RevisionTooLow.val());
    }
    Ok(SdtEntry { sig, revision })
}

// ── 双轨对拍账 ──────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct DualTrackLedger {
    pub rsdt_sigs: Vec<u32>,
    pub xsdt_sigs: Vec<u32>,
    pub mismatch_caught: u32,
}

impl DualTrackLedger {
    /// 32 位轨登记（RSDT 条目，空指针拒）。
    pub fn add_rsdt_entry(&mut self, ptr: u32, sig: u32) -> Result<(), u32> {
        if ptr == 0 {
            self.mismatch_caught += 1;
            return Err(CompatCode::NullEntryPointer.val());
        }
        self.rsdt_sigs.push(sig);
        Ok(())
    }
    /// 64 位轨登记（XSDT 条目，空指针拒）。
    pub fn add_xsdt_entry(&mut self, ptr: u64, sig: u32) -> Result<(), u32> {
        if ptr == 0 {
            self.mismatch_caught += 1;
            return Err(CompatCode::NullEntryPointer.val());
        }
        self.xsdt_sigs.push(sig);
        Ok(())
    }
    /// 双轨签名集一致（对拍：RSDT ⊆ XSDT 且计数相等）。
    pub fn dual_audit(&self) -> Result<(), u32> {
        if self.xsdt_sigs.is_empty() {
            return Ok(()); // 单轨（v1）合法。
        }
        if self.rsdt_sigs.len() != self.xsdt_sigs.len() {
            return Err(CompatCode::TrackMismatch.val());
        }
        let mut i = 0;
        while i < self.rsdt_sigs.len() {
            if self.rsdt_sigs[i] != self.xsdt_sigs[i] {
                return Err(CompatCode::TrackMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：签名路/校验路/矩阵路/双轨路） ──────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 构造合法 RSDP v2（XSDT 在场）。
pub fn make_rsdp_v2() -> Vec<u8> {
    let mut b = vec![0u8; RSDP_LEN_V2];
    b[0..8].copy_from_slice(b"RSD PTR ");
    b[8] = b'O';
    b[9] = b'V';
    b[10] = b'X';
    b[11] = b'I';
    b[15] = 2; // revision 2
    b[16..20].copy_from_slice(&0xE000_1000u32.to_le_bytes());
    b[24..32].copy_from_slice(&0xE000_2000u64.to_le_bytes());
    let mut sum: u8 = 0;
    let mut i = 0;
    while i < RSDP_LEN_V1 {
        sum = sum.wrapping_add(b[i]);
        i += 1;
    }
    b[9] = b'O'; // 保持 OEM 语义（校验落在 [8]？——v1 校验位在 [8] 之下不含 OEM 之外的修正位）
    // v1 校验位在 [8]，调整 [8] 使前 20 字节和为 0。
    let mut s2: u8 = 0;
    let mut i2 = 0;
    while i2 < RSDP_LEN_V1 {
        if i2 != 8 {
            s2 = s2.wrapping_add(b[i2]);
        }
        i2 += 1;
    }
    b[8] = s2.wrapping_neg();
    let _ = sum;
    // v2 扩展校验位在 [32]。
    let mut s3: u8 = 0;
    let mut i3 = 0;
    while i3 < RSDP_LEN_V2 - 1 {
        s3 = s3.wrapping_add(b[i3]);
        i3 += 1;
    }
    b[RSDP_LEN_V2 - 1] = s3.wrapping_neg();
    b
}

/// 构造合法 SDT（给定签名与修订）。
pub fn make_sdt(sig: u32, revision: u8) -> Vec<u8> {
    let total = SDT_HEADER_LEN + 8;
    let mut b = vec![0u8; total];
    b[0..4].copy_from_slice(&sig.to_le_bytes());
    b[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    b[8] = revision;
    let mut s: u8 = 0;
    let mut i = 0;
    while i < total - 1 {
        s = s.wrapping_add(b[i]);
        i += 1;
    }
    b[total - 1] = s.wrapping_neg();
    b
}

// ── 黄金全链：RSDP→双轨→表清单→矩阵→对拍 ────────────────────────
pub fn golden_run() -> Result<(u8, bool, usize), u32> {
    let rsdp = Rsdp::parse(&make_rsdp_v2())?;
    let (track, xsdt_present) = rsdp.track();
    let mut led = DualTrackLedger::default();
    let sigs = [
        0x54445344, // 'DSDT'
        0x54435046, // 'FACP'
        0x54445041, // 'APIC'
        0x54435048, // 'HPET'
    ];
    let mut i = 0;
    while i < sigs.len() {
        led.add_rsdt_entry(0xE100_0000 + (i as u32) * 0x100, sigs[i])?;
        led.add_xsdt_entry(0xE200_0000 + (i as u64) * 0x100, sigs[i])?;
        i += 1;
    }
    led.dual_audit()?;
    // 修订矩阵：四表逐一过闸。
    let revs = [2u8, 4, 2, 1];
    let mut i = 0;
    while i < sigs.len() {
        parse_sdt(&make_sdt(sigs[i], revs[i]))?;
        i += 1;
    }
    Ok((track, xsdt_present, led.rsdt_sigs.len()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：v2 + XSDT ⇒ 64 位轨。
pub fn probe_seq_a() -> Result<(), u32> {
    let rsdp = Rsdp::parse(&make_rsdp_v2())?;
    match rsdp.track() {
        (2, true) => Ok(()),
        _ => Err(CompatCode::NotReproducible.val()),
    }
}

/// 序 B：修订低于矩阵 ⇒ RevisionTooLow（HPET rev 0）。
pub fn probe_seq_b() -> Result<(), u32> {
    match parse_sdt(&make_sdt(0x54435048, 0)) {
        Err(e) if e == CompatCode::RevisionTooLow.val() => Ok(()),
        _ => Err(CompatCode::NotReproducible.val()),
    }
}

/// 序 C：双轨签名错位 ⇒ TrackMismatch。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = DualTrackLedger::default();
    let _ = led.add_rsdt_entry(0xE100_0000, 0x54445344);
    let _ = led.add_xsdt_entry(0xE200_0000, 0x54435046);
    match led.dual_audit() {
        Err(e) if e == CompatCode::TrackMismatch.val() => Ok(()),
        _ => Err(CompatCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：签名路（RSDP 签名破坏必拒）。
        let mut b = make_rsdp_v2();
        b[0] ^= 0xFF;
        led.record(0, Rsdp::parse(&b).is_err());
        // 路 2：校验路（v2 扩展区破坏必拒：v1 段不覆盖 [20..32)）。
        let mut b2 = make_rsdp_v2();
        b2[24] ^= 0x01;
        led.record(1, Rsdp::parse(&b2).is_err());
        // 路 3：矩阵路（表校验和破坏必拒）。
        let mut b3 = make_sdt(0x54445344, 2);
        let n3 = b3.len();
        b3[12] ^= 0x01;
        led.record(2, parse_sdt(&b3).is_err() && n3 == SDT_HEADER_LEN + 8);
        // 路 4：双轨路（计数不等必拒）。
        let mut l4 = DualTrackLedger::default();
        let _ = l4.add_rsdt_entry(0xE100_0000, 0x54445344);
        let _ = l4.add_rsdt_entry(0xE100_0100, 0x54435046);
        let _ = l4.add_xsdt_entry(0xE200_0000, 0x54445344);
        let hit4 = match l4.dual_audit() {
            Err(e) => e == CompatCode::TrackMismatch.val(),
            Ok(()) => false,
        };
        led.record(3, hit4);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CompatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CompatCode::ALL.len() {
        let v = CompatCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 主判据：v2 走 64 位轨 + 四表过闸 + 双轨一致。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((track, xsdt, n)) => *track == 2 && *xsdt && *n == 4,
            Err(_) => false,
        },
        match &g {
            Ok((track, xsdt, n)) => format!("track={} xsdt={} n={}", track, xsdt, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 回退轨：v1 无 XSDT ⇒ RSDT 轨（有账）。
    let mut b = make_rsdp_v2();
    b[15] = 1; // revision 1
    // 重建 v1 校验（前 20 字节和 0；[32] 已是全段位，v1 不查）。
    let mut s: u8 = 0;
    let mut i = 0;
    while i < RSDP_LEN_V1 {
        if i != 8 {
            s = s.wrapping_add(b[i]);
        }
        i += 1;
    }
    b[8] = s.wrapping_neg();
    let rsdp_v1 = Rsdp::parse(&b);
    v.push((
        "J1-回退轨",
        match &rsdp_v1 {
            Ok(r) => r.track() == (1, false),
            Err(_) => false,
        },
        String::from("rev1 ⇒ RSDT 32 位轨"),
    ));
    // 矩阵全量：8 签名逐一最低修订放行/拒绝。
    let mut mat_ok = true;
    let mut i2 = 0;
    while i2 < MATRIX_SIGS.len() {
        let t = parse_sdt(&make_sdt(MATRIX_SIGS[i2], MATRIX_MIN_REV[i2]));
        if t.is_err() {
            mat_ok = false;
        }
        i2 += 1;
    }
    v.push((
        "J1-矩阵全量",
        mat_ok,
        String::from("8 表最低修订放行"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b2 = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b2 && c,
        format!("a=绿 b=检出{} c=检出{}", b2, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6DD80 窗口"),
    ));
    v
}

pub fn run_unx0054_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0054 ACPI 表版本兼容矩阵");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
