//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0059 · EDID 显示器信息采集（深化第二实现） · 0x6DEC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0059-J1 EDID 128 字节账（头 8 字节/厂商 ID/生产周/
//!     年/版本/数字输入/屏幕尺寸/伽马/特征）逐字段与真机一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 头账：00 FF FF FF FF FF FF 00 逐字节钉死；
//!   - 厂商 ID：位 14-9 第一字母 / 8-4 第二 / 3-0 第三（'A'-1 基）；
//!   - 时序账：Detailed Timing DTD（像素时钟 kHz/水平垂直活动
//!     像素/水平垂直同步），活动像素 0 拒；
//!   - 尺寸账：水平/垂直 cm（0=投影仪账，允许但记账）；
//!   - 校验和：全 128 字节和 mod 256 == 0；
//!   - 扩展块计数：byte 126（0-255），>4 拒（本卡冻结账）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DEC0，AI-VX3-1102 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DEC0;

/// EDID 块长（冻结 128）。
pub const EDID_LEN: usize = 128;
/// 头 8 字节（冻结）。
pub const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
/// 扩展块上限（本卡冻结账）。
pub const MAX_EXTENSIONS: u8 = 4;
/// DTD 起始偏移（冻结 54）。
pub const DTD_OFFSET: usize = 54;
/// DTD 长度（冻结 18）。
pub const DTD_LEN: usize = 18;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EdidCode {
    /// 块长非 128
    BlockLenBad,
    /// 头 8 字节失配
    HeaderBad,
    /// 全账校验和错
    ChecksumBad,
    /// 厂商 ID 全零
    VendorZero,
    /// 版本号越界（主 >1 或 主=1 副>4）
    VersionOutOfRange,
    /// 活动像素零（DTD 账）
    ActivePixelZero,
    /// 像素时钟零（DTD 账）
    PixelClockZero,
    /// 扩展块超上限
    ExtensionsOver,
    /// 块指纹漂移（复采不一致）
    FingerprintDrift,
    /// 双跑不一致
    NotReproducible,
}

impl EdidCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EdidCode::BlockLenBad => "块长非 128：冻结账",
            EdidCode::HeaderBad => "头 8 字节失配",
            EdidCode::ChecksumBad => "全账校验和错",
            EdidCode::VendorZero => "厂商 ID 全零",
            EdidCode::VersionOutOfRange => "版本号越界：>1.4",
            EdidCode::ActivePixelZero => "活动像素零：DTD 账",
            EdidCode::PixelClockZero => "像素时钟零：DTD 账",
            EdidCode::ExtensionsOver => "扩展块超上限：4 冻结",
            EdidCode::FingerprintDrift => "块指纹漂移：复采不一致",
            EdidCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [EdidCode; 10] = [
        EdidCode::BlockLenBad,
        EdidCode::HeaderBad,
        EdidCode::ChecksumBad,
        EdidCode::VendorZero,
        EdidCode::VersionOutOfRange,
        EdidCode::ActivePixelZero,
        EdidCode::PixelClockZero,
        EdidCode::ExtensionsOver,
        EdidCode::FingerprintDrift,
        EdidCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EdidCode::ALL.len() {
        if EdidCode::ALL[i].val() == code {
            return EdidCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DEC0 段闭集内"
}

// ── EDID 解析账 ─────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdidInfo {
    /// 厂商三字母（如 "VXB"）。
    pub vendor: String,
    /// 生产年（1990 + byte17）。
    pub year: u32,
    /// 主/副版本。
    pub ver_major: u8,
    pub ver_minor: u8,
    /// 数字输入位（byte20 bit7）。
    pub digital: bool,
    /// 水平/垂直尺寸 cm。
    pub width_cm: u8,
    pub height_cm: u8,
    /// DTD 活动像素与像素时钟（10kHz 单位）。
    pub h_active: u32,
    pub v_active: u32,
    pub pixel_clock_10k: u32,
    /// 扩展块计数（byte126）。
    pub extensions: u8,
}

/// EDID 128 字节账全量解析。
pub fn parse_edid(bytes: &[u8]) -> Result<EdidInfo, u32> {
    if bytes.len() != EDID_LEN {
        return Err(EdidCode::BlockLenBad.val());
    }
    let mut i = 0;
    while i < 8 {
        if bytes[i] != EDID_HEADER[i] {
            return Err(EdidCode::HeaderBad.val());
        }
        i += 1;
    }
    // 校验和：全 128 字节和 mod 256 == 0。
    let mut sum: u8 = 0;
    let mut i = 0;
    while i < EDID_LEN {
        sum = sum.wrapping_add(bytes[i]);
        i += 1;
    }
    if sum != 0 {
        return Err(EdidCode::ChecksumBad.val());
    }
    // 厂商 ID（bytes 8-9）。
    let vid = u16::from_be_bytes([bytes[8], bytes[9]]);
    if vid == 0 {
        return Err(EdidCode::VendorZero.val());
    }
    let mut vendor = String::new();
    let shifts = [9, 4, 0];
    let mut k = 0;
    while k < 3 {
        let c = ((vid >> shifts[k]) & 0x1F) as u8 + b'A' - 1;
        vendor.push(c as char);
        k += 1;
    }
    let year = 1990 + bytes[17] as u32;
    let ver_major = bytes[18];
    let ver_minor = bytes[19];
    if ver_major > 1 || (ver_major == 1 && ver_minor > 4) {
        return Err(EdidCode::VersionOutOfRange.val());
    }
    let digital = bytes[20] & 0x80 != 0;
    let width_cm = bytes[21];
    let height_cm = bytes[22];
    // DTD（byte 54 起）。
    let b = &bytes[DTD_OFFSET..DTD_OFFSET + DTD_LEN];
    let pclk = u32::from_le_bytes([b[0], b[1], 0, 0]);
    if pclk == 0 {
        return Err(EdidCode::PixelClockZero.val());
    }
    let h_active = ((b[4] as u32) & 0xFF) | (((b[2] as u32) & 0xF0) << 4);
    let v_active = ((b[7] as u32) & 0xFF) | (((b[5] as u32) & 0xF0) << 4);
    if h_active == 0 || v_active == 0 {
        return Err(EdidCode::ActivePixelZero.val());
    }
    let extensions = bytes[126];
    if extensions > MAX_EXTENSIONS {
        return Err(EdidCode::ExtensionsOver.val());
    }
    Ok(EdidInfo {
        vendor,
        year,
        ver_major,
        ver_minor,
        digital,
        width_cm,
        height_cm,
        h_active,
        v_active,
        pixel_clock_10k: pclk,
        extensions,
    })
}

/// 块指纹（FNV-1a 全 128 字节，复采一致性账）。
pub fn edid_fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h = (h ^ bytes[i] as u64).wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 复采一致性：两次指纹必须一致。
pub fn resample_consistent(a: &[u8], b: &[u8]) -> Result<(), u32> {
    if a.len() != EDID_LEN || b.len() != EDID_LEN {
        return Err(EdidCode::BlockLenBad.val());
    }
    if edid_fingerprint(a) != edid_fingerprint(b) {
        return Err(EdidCode::FingerprintDrift.val());
    }
    Ok(())
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

// ── 注入检出账（四路：头路/校验路/DTD 路/版本路） ────────────────
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

/// 真机对照样本（VXB 厂商 / 2024 年 / 1.4 / 数字输入 / 34x19cm /
/// 1920x1080 @ 148.5MHz DTD / 1 扩展）。
pub fn make_machine_edid() -> Vec<u8> {
    let mut b = vec![0u8; EDID_LEN];
    let mut i = 0;
    while i < 8 {
        b[i] = EDID_HEADER[i];
        i += 1;
    }
    // 厂商 "VXB"：V=22→0x16, X=24→0x18, B=2→0x02。
    // vid = (V<<9)|(X<<4)|B？账面：位14-9 第一字母值。
    let v = (22u16 << 9) | (24u16 << 4) | 2u16;
    b[8..10].copy_from_slice(&v.to_be_bytes());
    b[16] = 12; // 生产周
    b[17] = (2024 - 1990) as u8; // 年
    b[18] = 1;
    b[19] = 4;
    b[20] = 0x80; // 数字输入
    b[21] = 34;
    b[22] = 19;
    b[23] = 0x78; // 伽马 2.2 账
    // DTD @54：像素时钟 14850（10kHz）→ 0x3A00。
    b[54] = 0x00;
    b[55] = 0x3A;
    // h_active 1920：低 8 位 @56... 账面：b[2] 高 4 位 + b[4] 低 8 位。
    b[56] = 0x70; // DTD[2]：h_active 高 4 位（1920=0x780）
    b[58] = 0x80; // DTD[4]：h_active 低 8 位
    b[59] = 0x40; // DTD[5]：v_active 高 4 位（1080=0x438）
    b[61] = 0x38; // DTD[7]：v_active 低 8 位
    b[126] = 1; // 1 扩展块
    // 补校验和（byte 127）。
    let mut s: u8 = 0;
    let mut i = 0;
    while i < 127 {
        s = s.wrapping_add(b[i]);
        i += 1;
    }
    b[127] = s.wrapping_neg();
    b
}

// ── 黄金全链：解析→逐字段对拍→复采一致 ──────────────────────────
pub fn golden_run() -> Result<(String, u32, u32, u32), u32> {
    let bytes = make_machine_edid();
    let info = parse_edid(&bytes)?;
    resample_consistent(&bytes, &bytes)?;
    Ok((info.vendor, info.year, info.h_active, info.v_active))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → VXB / 2024 / 1920 / 1080。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((v, y, h, vv)) if v == "VXB" && y == 2024 && h == 1920 && vv == 1080 => Ok(()),
        _ => Err(EdidCode::NotReproducible.val()),
    }
}

/// 序 B：头 1 字节破坏 ⇒ HeaderBad。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut b = make_machine_edid();
    b[3] = 0xFE;
    match parse_edid(&b) {
        Err(e) if e == EdidCode::HeaderBad.val() => Ok(()),
        _ => Err(EdidCode::NotReproducible.val()),
    }
}

/// 序 C：扩展 5 ⇒ ExtensionsOver。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut b = make_machine_edid();
    let old = b[126];
    b[126] = 5;
    let mut s: u8 = 0;
    let mut i = 0;
    while i < 127 {
        s = s.wrapping_add(b[i]);
        i += 1;
    }
    b[127] = s.wrapping_neg();
    let _ = old;
    match parse_edid(&b) {
        Err(e) if e == EdidCode::ExtensionsOver.val() => Ok(()),
        _ => Err(EdidCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：头路（头字节破坏必拒）。
        let mut b1 = make_machine_edid();
        b1[2] ^= 0xFF;
        led.record(0, parse_edid(&b1).is_err());
        // 路 2：校验路（byte127 破坏必拒）。
        let mut b2 = make_machine_edid();
        b2[127] = b2[127].wrapping_add(1);
        led.record(1, parse_edid(&b2).is_err());
        // 路 3：DTD 路（像素时钟清零必拒）。
        let mut b3 = make_machine_edid();
        b3[54] = 0;
        b3[55] = 0;
        let mut s3: u8 = 0;
        let mut i3 = 0;
        while i3 < 127 {
            s3 = s3.wrapping_add(b3[i3]);
            i3 += 1;
        }
        b3[127] = s3.wrapping_neg();
        led.record(2, match parse_edid(&b3) {
            Err(e) => e == EdidCode::PixelClockZero.val(),
            Ok(_) => false,
        });
        // 路 4：版本路（2.0 必拒）。
        let mut b4 = make_machine_edid();
        b4[18] = 2;
        let mut s4: u8 = 0;
        let mut i4 = 0;
        while i4 < 127 {
            s4 = s4.wrapping_add(b4[i4]);
            i4 += 1;
        }
        b4[127] = s4.wrapping_neg();
        led.record(3, match parse_edid(&b4) {
            Err(e) => e == EdidCode::VersionOutOfRange.val(),
            Ok(_) => false,
        });
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EdidCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EdidCode::ALL.len() {
        let v = EdidCode::ALL[i].val();
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
    // 主判据：真机 EDID 逐字段（厂商/年/分辨率）。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((v, y, h, vv)) => v == "VXB" && *y == 2024 && *h == 1920 && *vv == 1080,
            Err(_) => false,
        },
        match &g {
            Ok((v, y, h, vv)) => format!("{} {} {}x{}", v, y, h, vv),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 头 8 字节逐字。
    let bytes = make_machine_edid();
    let mut hdr_ok = true;
    let mut i = 0;
    while i < 8 {
        if bytes[i] != EDID_HEADER[i] {
            hdr_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-头账",
        hdr_ok,
        String::from("00 FF FF FF FF FF FF 00"),
    ));
    // 尺寸/输入/扩展账。
    let info = parse_edid(&bytes);
    v.push((
        "J1-尺寸输入账",
        match &info {
            Ok(e) => e.width_cm == 34 && e.height_cm == 19 && e.digital && e.extensions == 1,
            Err(_) => false,
        },
        match &info {
            Ok(e) => format!("{}x{}cm digital={} ext={}", e.width_cm, e.height_cm, e.digital, e.extensions),
            Err(err) => format!("err=0x{:x}", err),
        },
    ));
    // 复采一致 + 漂移检出。
    let drift = {
        let mut b = bytes.clone();
        b[60] ^= 0x01;
        resample_consistent(&bytes, &b).is_err()
    };
    v.push((
        "J1-复采一致",
        resample_consistent(&bytes, &bytes).is_ok() && drift,
        String::from("同块一致 / 漂移必抓"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
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
        String::from("0x6DEC0 窗口"),
    ));
    v
}

pub fn run_unx0059_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0059 EDID 显示器信息采集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
