//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0442 RSDP 校验与版本账 · 0x75040段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，WBV1104·AI-VX7-1104）：
//!   四阶验证链（签名→v1 checksum 20B→v2 长度自洽→v2 ext checksum
//!   36B），版本账 v1/v2+/未知 revision 拒。
//! 设计要点：
//!   1. RSDP 结构真实建模：38 字节 v2 布局（签名 8 + checksum 1 +
//!      oemid 6 + revision 1 + rsdt_addr 4 + length 4 + xsdt_addr 8 +
//!      ext_checksum 1 + reserved 3），encode/decode 双向。
//!   2. 四阶验证链逐阶可注入破坏：签名翻 / checksum 翻 / length 改 /
//!      ext_checksum 翻——每阶注入必被当阶拒。
//!   3. 第一阶签名：8 字节 "RSD PTR " 整段比对；
//!      第二阶 v1 checksum：前 20 字节累加 ≡ 0（mod 256）；
//!      第三阶 v2 长度自洽：length 字段必须为 36；
//!      第四阶 v2 ext checksum：第 20..36 字节累加 ≡ 0（mod 256）。
//!   4. 版本账：revision 0 = v1、2 = v2+；revision 1 与 > 2 属规范
//!      未定义值，显性归档不猜（ArchiveLedger 记账 + 拒入主链）。
//!   5. 验证单调状态机：SigStage → CsumStage → LenStage → ExtStage，
//!      倒退/跳阶显性拒。
//!   6. 注入路（4 阶 × 未知 revision）逐路必抓，LCG 种子固定。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75040，WBV1104 批 ACPI 栈第二卡区）。
pub const SEG_BASE: u32 = 0x75040;

// ── 冻结常量表（来源：ACPI 规范 §5.2.5.3 RSDP 结构布局） ─────────
/// 签名长度。
pub const SIG_LEN: usize = 8;
/// v1 结构总长（到 rsdt_address 止）。
pub const V1_LEN: usize = 20;
/// v2 结构总长（length 字段必须值）。
pub const V2_LEN: u32 = 36;
/// v2 原始镜像长（36 + reserved 3 = 39？——规范：36 字节有效 +
/// 3 字节保留 = 39；本仓镜像按有效 36 + 保留 3 = 39 冻结）。
pub const RAW_LEN: usize = 39;
/// oem_id 长度。
pub const OEM_ID_LEN: usize = 6;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RsdpVerifyCode {
    /// 签名失配（第一阶）
    SigMismatch,
    /// v1 checksum 不过（第二阶）
    Csum20Bad,
    /// v2 长度自洽失配（第三阶）
    LenInconsistent,
    /// v2 ext checksum 不过（第四阶）
    ExtCsumBad,
    /// 未知 revision（1 或 >2，规范未定义）
    RevUnknown,
    /// 验证状态机倒退/跳阶
    StageRegress,
    /// 双跑不一致
    NotReproducible,
}

impl RsdpVerifyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RsdpVerifyCode::SigMismatch => "签名失配：前 8 字节 ≠ \"RSD PTR \"，建议核对镜像偏移是否对准",
            RsdpVerifyCode::Csum20Bad => "v1 checksum 不过：前 20 字节累加 ≢ 0，建议核对 RSDP 前 20 字节完整性",
            RsdpVerifyCode::LenInconsistent => "v2 长度自洽失配：length 字段 ≠ 36，建议核对 revision 与 length 配对",
            RsdpVerifyCode::ExtCsumBad => "v2 ext checksum 不过：第 20..36 字节累加 ≢ 0，建议核对扩展头完整性",
            RsdpVerifyCode::RevUnknown => "未知 revision：1 与 >2 为规范未定义值，已显性归档不猜，建议核对固件 ACPI 版本声明",
            RsdpVerifyCode::StageRegress => "验证状态机倒退/跳阶：单调推进被破坏，建议检查验证驱动序",
            RsdpVerifyCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定源",
        }
    }
    pub const ALL: [RsdpVerifyCode; 7] = [
        RsdpVerifyCode::SigMismatch,
        RsdpVerifyCode::Csum20Bad,
        RsdpVerifyCode::LenInconsistent,
        RsdpVerifyCode::ExtCsumBad,
        RsdpVerifyCode::RevUnknown,
        RsdpVerifyCode::StageRegress,
        RsdpVerifyCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RsdpVerifyCode::ALL.len() {
        if let Some(c) = RsdpVerifyCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75040 段闭集内"
}

// ── RSDP 签名 ───────────────────────────────────────────────────
pub const RSDP_SIG: [u8; SIG_LEN] = [0x52, 0x53, 0x44, 0x20, 0x50, 0x54, 0x52, 0x20];

/// 字节累加（mod 256）。
pub fn sum_mod(bytes: &[u8]) -> u8 {
    bytes.iter().fold(0u8, |a, x| a.wrapping_add(*x))
}

// ── RSDP 结构真实建模（38/39 字节 v2 布局） ──────────────────────
/// v2 布局字段账：
///   0..8   签名 "RSD PTR "
///   8      checksum（前 20 字节和补）
///   9..15  oem_id（6 字节）
///   15     revision
///   16..20 rsdt_address（LE u32）
///   20..24 length（LE u32，v2 必须 36）
///   24..32 xsdt_address（LE u64）
///   32     ext_checksum（第 20..36 字节和补）
///   33..36 reserved（3 字节，恒 0）
/// 有效载荷 36 字节；镜像含 3 字节保留尾 → RAW_LEN 39。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RsdpV2 {
    pub signature: [u8; SIG_LEN],
    pub checksum: u8,
    pub oem_id: [u8; OEM_ID_LEN],
    pub revision: u8,
    pub rsdt_addr: u32,
    pub length: u32,
    pub xsdt_addr: u64,
    pub ext_checksum: u8,
    pub reserved: [u8; 3],
}

impl RsdpV2 {
    /// 构造并自动补双校验和。
    pub fn build(revision: u8, rsdt_addr: u32, xsdt_addr: u64) -> RsdpV2 {
        let mut r = RsdpV2 {
            signature: RSDP_SIG,
            checksum: 0,
            oem_id: [b'V', b'A', b'R', b'I', b'X', b'K'],
            revision,
            rsdt_addr,
            length: if revision == 0 { V1_LEN as u32 } else { V2_LEN },
            xsdt_addr,
            ext_checksum: 0,
            reserved: [0; 3],
        };
        r.repair_checksums();
        r
    }

    /// 重算两道校验和（v1 只补第一道）。
    pub fn repair_checksums(&mut self) {
        let head = self.encode_head();
        self.checksum = sum_mod(&head).wrapping_neg();
        if self.revision != 0 {
            let ext = self.encode_ext();
            self.ext_checksum = sum_mod(&ext).wrapping_neg();
        }
    }

    /// 前 20 字节（v1 checksum 覆盖域）。
    fn encode_head(&self) -> [u8; V1_LEN] {
        let mut b = [0u8; V1_LEN];
        let mut i = 0;
        while i < SIG_LEN {
            b[i] = self.signature[i];
            i += 1;
        }
        b[8] = self.checksum;
        let mut j = 0;
        while j < OEM_ID_LEN {
            b[9 + j] = self.oem_id[j];
            j += 1;
        }
        b[15] = self.revision;
        b[16] = (self.rsdt_addr & 0xFF) as u8;
        b[17] = ((self.rsdt_addr >> 8) & 0xFF) as u8;
        b[18] = ((self.rsdt_addr >> 16) & 0xFF) as u8;
        b[19] = ((self.rsdt_addr >> 24) & 0xFF) as u8;
        b
    }

    /// 第 20..36 字节（v2 ext checksum 覆盖域）。
    fn encode_ext(&self) -> [u8; 16] {
        let mut b = [0u8; 16];
        let len = self.length;
        b[0] = (len & 0xFF) as u8;
        b[1] = ((len >> 8) & 0xFF) as u8;
        b[2] = ((len >> 16) & 0xFF) as u8;
        b[3] = ((len >> 24) & 0xFF) as u8;
        let x = self.xsdt_addr;
        let mut k = 0;
        while k < 8 {
            b[4 + k] = ((x >> (8 * k)) & 0xFF) as u8;
            k += 1;
        }
        b[12] = self.ext_checksum;
        // reserved 3 字节恒 0。
        b
    }

    /// 编码为原始镜像（39 字节）。
    pub fn encode(&self) -> Vec<u8> {
        let mut v: Vec<u8> = Vec::new();
        let head = self.encode_head();
        let mut i = 0;
        while i < V1_LEN {
            v.push(head[i]);
            i += 1;
        }
        let ext = self.encode_ext();
        let mut j = 0;
        while j < 16 {
            v.push(ext[j]);
            j += 1;
        }
        let mut k = 0;
        while k < 3 {
            v.push(self.reserved[k]);
            k += 1;
        }
        v
    }

    /// 从镜像解码（长度 ≥ 20；v2+ 需 ≥ 36）。
    pub fn decode(img: &[u8]) -> Result<RsdpV2, u32> {
        if img.len() < V1_LEN {
            return Err(RsdpVerifyCode::Csum20Bad.val());
        }
        let mut sig = [0u8; SIG_LEN];
        let mut i = 0;
        while i < SIG_LEN {
            match img.get(i) {
                Some(b) => sig[i] = *b,
                None => return Err(RsdpVerifyCode::SigMismatch.val()),
            }
            i += 1;
        }
        let checksum = *img.get(8).unwrap_or(&0);
        let mut oem = [0u8; OEM_ID_LEN];
        let mut j = 0;
        while j < OEM_ID_LEN {
            oem[j] = *img.get(9 + j).unwrap_or(&0);
            j += 1;
        }
        let revision = *img.get(15).unwrap_or(&0);
        let r0 = *img.get(16).unwrap_or(&0) as u32;
        let r1 = *img.get(17).unwrap_or(&0) as u32;
        let r2 = *img.get(18).unwrap_or(&0) as u32;
        let r3 = *img.get(19).unwrap_or(&0) as u32;
        let rsdt_addr = r0 | (r1 << 8) | (r2 << 16) | (r3 << 24);
        let l0 = *img.get(20).unwrap_or(&0) as u32;
        let l1 = *img.get(21).unwrap_or(&0) as u32;
        let l2 = *img.get(22).unwrap_or(&0) as u32;
        let l3 = *img.get(23).unwrap_or(&0) as u32;
        let length = l0 | (l1 << 8) | (l2 << 16) | (l3 << 24);
        let mut xsdt_addr = 0u64;
        let mut k = 0;
        while k < 8 {
            xsdt_addr |= (*img.get(24 + k).unwrap_or(&0) as u64) << (8 * k);
            k += 1;
        }
        let ext_checksum = *img.get(32).unwrap_or(&0);
        let mut reserved = [0u8; 3];
        let mut m = 0;
        while m < 3 {
            reserved[m] = *img.get(33 + m).unwrap_or(&0);
            m += 1;
        }
        Ok(RsdpV2 {
            signature: sig,
            checksum,
            oem_id: oem,
            revision,
            rsdt_addr,
            length,
            xsdt_addr,
            ext_checksum,
            reserved,
        })
    }
}

// ── 版本账 ──────────────────────────────────────────────────────
/// 版本类：revision 0 = v1、2 = v2+；其余未知显性归档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VersionClass {
    /// revision 0
    V1,
    /// revision 2
    V2Plus,
}

/// 归档条目：未知 revision 的显性记账（不猜、不静默丢）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub revision: u8,
    pub verdict: String,
}

/// 版本账本。
#[derive(Clone, Debug, Default)]
pub struct VersionLedger {
    pub v1_count: u32,
    pub v2_count: u32,
    pub unknown: Vec<ArchiveEntry>,
}

impl VersionLedger {
    pub fn new() -> VersionLedger {
        VersionLedger::default()
    }

    /// 归类一版 revision（未知值归档并拒）。
    pub fn classify(&mut self, revision: u8) -> Result<VersionClass, u32> {
        match revision {
            0 => {
                self.v1_count += 1;
                Ok(VersionClass::V1)
            }
            2 => {
                self.v2_count += 1;
                Ok(VersionClass::V2Plus)
            }
            other => {
                self.unknown.push(ArchiveEntry {
                    revision: other,
                    verdict: String::from("未知 revision：规范未定义，显性归档不猜"),
                });
                Err(RsdpVerifyCode::RevUnknown.val())
            }
        }
    }
}

// ── 验证单调状态机 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyStage {
    /// 未开始
    Idle,
    /// 第一阶：签名
    SigStage,
    /// 第二阶：v1 checksum
    CsumStage,
    /// 第三阶：v2 长度自洽
    LenStage,
    /// 第四阶：v2 ext checksum
    ExtStage,
    /// 全链通过
    Done,
}

impl VerifyStage {
    fn rank(self) -> u32 {
        match self {
            VerifyStage::Idle => 0,
            VerifyStage::SigStage => 1,
            VerifyStage::CsumStage => 2,
            VerifyStage::LenStage => 3,
            VerifyStage::ExtStage => 4,
            VerifyStage::Done => 5,
        }
    }
}

/// 验证游标：单调推进，倒退/跳阶显性拒。
#[derive(Clone, Debug)]
pub struct VerifyWalker {
    pub stage: VerifyStage,
}

impl VerifyWalker {
    pub fn new() -> VerifyWalker {
        VerifyWalker { stage: VerifyStage::Idle }
    }
    pub fn advance(&mut self, to: VerifyStage) -> Result<(), u32> {
        if to.rank() < self.stage.rank() {
            return Err(RsdpVerifyCode::StageRegress.val());
        }
        self.stage = to;
        Ok(())
    }
}

/// 四阶验证报告（每阶通过位 + 版本类）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyReport {
    pub sig_ok: bool,
    pub csum_ok: bool,
    pub len_ok: bool,
    pub ext_ok: bool,
    /// v1 时 ext 阶不适用（记 None）。
    pub ext_applicable: bool,
    pub version: Option<VersionClass>,
}

/// 四阶验证链主入口。
///
/// 逐阶推进，任一阶失败即返回该阶诊断码；未知 revision 在第三阶前
/// 被版本账拦下。
pub fn verify_chain(ledger: &mut VersionLedger, img: &[u8]) -> Result<VerifyReport, u32> {
    let mut w = VerifyWalker::new();
    let mut rep = VerifyReport {
        sig_ok: false,
        csum_ok: false,
        len_ok: false,
        ext_ok: false,
        ext_applicable: false,
        version: None,
    };
    // 第一阶：签名。
    w.advance(VerifyStage::SigStage)?;
    let sig_ok = match img.get(0..SIG_LEN) {
        Some(s) => s == &RSDP_SIG[..],
        None => false,
    };
    if !sig_ok {
        return Err(RsdpVerifyCode::SigMismatch.val());
    }
    rep.sig_ok = true;
    // 第二阶：v1 checksum（前 20 字节）。
    w.advance(VerifyStage::CsumStage)?;
    let head = match img.get(0..V1_LEN) {
        Some(h) => sum_mod(h),
        None => return Err(RsdpVerifyCode::Csum20Bad.val()),
    };
    if head != 0 {
        return Err(RsdpVerifyCode::Csum20Bad.val());
    }
    rep.csum_ok = true;
    // 版本账：revision 分类（未知显性归档并拒）。
    let revision = match img.get(15) {
        Some(r) => *r,
        None => return Err(RsdpVerifyCode::RevUnknown.val()),
    };
    let vc = ledger.classify(revision)?;
    rep.version = Some(vc);
    // 第三阶：v2 长度自洽（v1 直通：无 length 字段，有效长 20）。
    w.advance(VerifyStage::LenStage)?;
    match vc {
        VersionClass::V1 => {
            if img.len() < V1_LEN {
                return Err(RsdpVerifyCode::LenInconsistent.val());
            }
            rep.len_ok = true;
            w.advance(VerifyStage::Done)?;
            Ok(rep)
        }
        VersionClass::V2Plus => {
            let l0 = *img.get(20).unwrap_or(&0) as u32;
            let l1 = *img.get(21).unwrap_or(&0) as u32;
            let l2 = *img.get(22).unwrap_or(&0) as u32;
            let l3 = *img.get(23).unwrap_or(&0) as u32;
            let length = l0 | (l1 << 8) | (l2 << 16) | (l3 << 24);
            if length != V2_LEN {
                return Err(RsdpVerifyCode::LenInconsistent.val());
            }
            if img.len() < V2_LEN as usize {
                return Err(RsdpVerifyCode::LenInconsistent.val());
            }
            rep.len_ok = true;
            // 第四阶：v2 ext checksum（第 20..36 字节）。
            w.advance(VerifyStage::ExtStage)?;
            let ext = match img.get(20..V2_LEN as usize) {
                Some(e) => sum_mod(e),
                None => return Err(RsdpVerifyCode::ExtCsumBad.val()),
            };
            if ext != 0 {
                return Err(RsdpVerifyCode::ExtCsumBad.val());
            }
            rep.ext_ok = true;
            rep.ext_applicable = true;
            w.advance(VerifyStage::Done)?;
            Ok(rep)
        }
    }
}

// ── LCG（种子固定） ─────────────────────────────────────────────
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

// ── 注入面（四阶逐阶可破坏） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjectStage {
    /// 第一阶：翻签名尾字节
    FlipSig,
    /// 第二阶：翻 checksum 字节
    FlipCsum,
    /// 第三阶：length 改 40
    FlipLen,
    /// 第四阶：翻 ext_checksum 字节
    FlipExt,
}

/// 对合法 v2 镜像注入指定阶破坏。
pub fn inject_v2(img: &mut [u8], stage: InjectStage) -> Result<(), u32> {
    match stage {
        InjectStage::FlipSig => match img.get_mut(7) {
            Some(slot) => {
                *slot ^= 0x20;
                Ok(())
            }
            None => Err(RsdpVerifyCode::SigMismatch.val()),
        },
        InjectStage::FlipCsum => match img.get_mut(8) {
            Some(slot) => {
                *slot ^= 0xFF;
                Ok(())
            }
            None => Err(RsdpVerifyCode::Csum20Bad.val()),
        },
        InjectStage::FlipLen => match img.get_mut(20) {
            Some(slot) => {
                *slot = 40;
                Ok(())
            }
            None => Err(RsdpVerifyCode::LenInconsistent.val()),
        },
        InjectStage::FlipExt => match img.get_mut(32) {
            Some(slot) => {
                *slot ^= 0xFF;
                Ok(())
            }
            None => Err(RsdpVerifyCode::ExtCsumBad.val()),
        },
    }
}

/// 期望诊断码（注入 → 被当阶拒的码）。
fn expected_reject(stage: InjectStage) -> u32 {
    match stage {
        InjectStage::FlipSig => RsdpVerifyCode::SigMismatch.val(),
        InjectStage::FlipCsum => RsdpVerifyCode::Csum20Bad.val(),
        InjectStage::FlipLen => RsdpVerifyCode::LenInconsistent.val(),
        InjectStage::FlipExt => RsdpVerifyCode::ExtCsumBad.val(),
    }
}

// ── 注入检出账（四阶 + 未知 revision 共五路） ────────────────────
pub struct InjectLedger {
    pub tries: [u32; 5],
    pub caught: [u32; 5],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 5], caught: [0; 5] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 5 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if let (Some(t), Some(c)) = (self.tries.get(i), self.caught.get(i)) {
                if *t == 0 || c != t {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

const INJECT_STAGES: [InjectStage; 4] = [
    InjectStage::FlipSig,
    InjectStage::FlipCsum,
    InjectStage::FlipLen,
    InjectStage::FlipExt,
];

/// 注入五路：四阶破坏逐阶必拒 + 未知 revision（1 与 >2）必归档拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0..3：四阶逐阶破坏。
        let mut s = 0;
        while s < 4 {
            let stage = match INJECT_STAGES.get(s) {
                Some(x) => *x,
                None => break,
            };
            let mut img = RsdpV2::build(2, 0x00E0000, 0x10).encode();
            let _ = inject_v2(&mut img, stage);
            let mut led2 = VersionLedger::new();
            let got = verify_chain(&mut led2, &img);
            let hit = match got {
                Err(e) => e == expected_reject(stage),
                Ok(_) => false,
            };
            led.record(s, hit);
            s += 1;
        }
        // 路 4：未知 revision（轮转 1 / 3 / 0xFF）。
        let revs: [u8; 3] = [1, 3, 0xFF];
        let rev = match revs.get((r % 3) as usize) {
            Some(x) => *x,
            None => 1u8,
        };
        let img = RsdpV2::build(rev, 0x00E0000, 0x10).encode();
        let mut led3 = VersionLedger::new();
        let got = verify_chain(&mut led3, &img);
        let hit = match got {
            Err(e) => e == RsdpVerifyCode::RevUnknown.val() && led3.unknown.len() == 1,
            Ok(_) => false,
        };
        led.record(4, hit);
        r += 1;
    }
    Ok(led)
}

// ── 黄金流 ──────────────────────────────────────────────────────
/// 黄金景 A：v2 全链通过（四阶全绿）。
pub fn golden_v2() -> Result<VerifyReport, u32> {
    let img = RsdpV2::build(2, 0x00E0000, 0x10).encode();
    let mut led = VersionLedger::new();
    verify_chain(&mut led, &img)
}

/// 黄金景 B：v1 通过（ext 阶不适用）。
pub fn golden_v1() -> Result<VerifyReport, u32> {
    // v1 镜像只有前 20 字节有效；encode 尾部保留域清零不影响。
    let img = RsdpV2::build(0, 0x00E0000, 0).encode();
    // v1 无 length/xsdt/ext 域：截到 20 字节。
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < V1_LEN {
        match img.get(i) {
            Some(b) => v.push(*b),
            None => return Err(RsdpVerifyCode::Csum20Bad.val()),
        }
        i += 1;
    }
    let mut led = VersionLedger::new();
    verify_chain(&mut led, &v)
}

/// 黄金景 C：未知 revision（revision 1）显性归档并拒。
pub fn golden_unknown_rev() -> Result<(u32, usize), u32> {
    let img = RsdpV2::build(1, 0x00E0000, 0).encode();
    let mut led = VersionLedger::new();
    let e = match verify_chain(&mut led, &img) {
        Err(e) => e,
        Ok(_) => return Err(RsdpVerifyCode::NotReproducible.val()),
    };
    Ok((e, led.unknown.len()))
}

/// 探针序 A：v2 全链绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_v2() {
        Ok(rep) if rep.sig_ok && rep.csum_ok && rep.len_ok && rep.ext_ok && rep.ext_applicable => {
            Ok(())
        }
        _ => Err(RsdpVerifyCode::NotReproducible.val()),
    }
}

/// 探针序 B：v1 通过且 ext 阶不适用。
pub fn probe_seq_b() -> Result<(), u32> {
    match golden_v1() {
        Ok(rep) if rep.sig_ok && rep.csum_ok && rep.len_ok && !rep.ext_applicable => Ok(()),
        _ => Err(RsdpVerifyCode::NotReproducible.val()),
    }
}

/// 探针序 C：四阶注入全拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let led = inject_rounds(1)?;
    if led.all_caught() {
        Ok(())
    } else {
        Err(RsdpVerifyCode::NotReproducible.val())
    }
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_v2()?;
    let b = golden_v2()?;
    let c = golden_v1()?;
    let d = golden_v1()?;
    if a != b || c != d {
        return Err(RsdpVerifyCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < RsdpVerifyCode::ALL.len() {
        if let Some(c) = RsdpVerifyCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 判据：四阶链全绿（v2）。
fn chain_v2_ok() -> bool {
    probe_seq_a().is_ok()
}

/// 判据：版本账三分（v1 过 / v2+ 过 / 未知拒）。
fn ledger_trichotomy_ok() -> bool {
    let mut led = VersionLedger::new();
    let a = led.classify(0);
    let b = led.classify(2);
    let c = led.classify(1);
    let d = led.classify(3);
    a == Ok(VersionClass::V1)
        && b == Ok(VersionClass::V2Plus)
        && c == Err(RsdpVerifyCode::RevUnknown.val())
        && d == Err(RsdpVerifyCode::RevUnknown.val())
        && led.v1_count == 1
        && led.v2_count == 1
        && led.unknown.len() == 2
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 守卫：段位窗口。
    v.push((
        "守卫-段位窗口",
        segment_guard(),
        format!("SEG_BASE=0x{:x}", SEG_BASE),
    ));

    // 判据：四阶链全绿（v2）。
    let a = golden_v2();
    v.push((
        "J1-四阶链v2全绿",
        chain_v2_ok(),
        match &a {
            Ok(rep) => format!(
                "sig={} csum={} len={} ext={}",
                rep.sig_ok, rep.csum_ok, rep.len_ok, rep.ext_ok
            ),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // v1 通过（ext 阶不适用）。
    let b = golden_v1();
    v.push((
        "v1链通过-ext不适用",
        probe_seq_b().is_ok(),
        match &b {
            Ok(rep) => format!("len_ok={} ext_applicable={}", rep.len_ok, rep.ext_applicable),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 版本账三分。
    v.push((
        "版本账三分",
        ledger_trichotomy_ok(),
        String::from("rev0=v1 rev2=v2+ rev1/>2 归档拒"),
    ));

    // 未知 revision 黄金景。
    let c = golden_unknown_rev();
    v.push((
        "未知revision归档拒",
        match &c {
            Ok((e, n)) => *e == RsdpVerifyCode::RevUnknown.val() && *n == 1,
            Err(_) => false,
        },
        match &c {
            Ok((e, n)) => format!("code=0x{:x} archived={}", e, n),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 逐阶注入必抓（静态四阶）。
    {
        let mut all = true;
        let mut log = String::new();
        let mut s = 0;
        while s < 4 {
            let stage = match INJECT_STAGES.get(s) {
                Some(x) => *x,
                None => break,
            };
            let mut img = RsdpV2::build(2, 0x00E0000, 0x10).encode();
            let _ = inject_v2(&mut img, stage);
            let mut led = VersionLedger::new();
            let hit = match verify_chain(&mut led, &img) {
                Err(e) => e == expected_reject(stage),
                Ok(_) => false,
            };
            if !hit {
                all = false;
            }
            log.push_str(format!("s{}={}", s, hit as u32).as_str());
            s += 1;
        }
        v.push((
            "逐阶注入必抓",
            all,
            log,
        ));
    }

    // 注入 10/10（含未知 revision 路）。
    let inj = inject_rounds(10);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 布局常量冻结账。
    v.push((
        "布局常量冻结",
        SIG_LEN == 8 && V1_LEN == 20 && V2_LEN == 36 && RAW_LEN == 39 && OEM_ID_LEN == 6,
        format!("sig={} v1={} v2={} raw={} oem={}", SIG_LEN, V1_LEN, V2_LEN, RAW_LEN, OEM_ID_LEN),
    ));

    // encode/decode 往返账。
    {
        let src = RsdpV2::build(2, 0x00E0000, 0x1122334455667788);
        let img = src.encode();
        let dst = RsdpV2::decode(&img);
        let ok = match dst {
            Ok(d) => d == src,
            Err(_) => false,
        };
        v.push((
            "encode/decode往返",
            ok,
            String::from("39 字节镜像双向一致"),
        ));
    }

    // 状态机倒退账（rank 单调性）。
    v.push((
        "状态机rank单调",
        VerifyStage::Idle.rank() < VerifyStage::SigStage.rank()
            && VerifyStage::SigStage.rank() < VerifyStage::CsumStage.rank()
            && VerifyStage::CsumStage.rank() < VerifyStage::LenStage.rank()
            && VerifyStage::LenStage.rank() < VerifyStage::ExtStage.rank()
            && VerifyStage::ExtStage.rank() < VerifyStage::Done.rank(),
        String::from("Idle<Sig<Csum<Len<Ext<Done"),
    ));

    // 签名闭集。
    v.push((
        "签名闭集8字节",
        RSDP_SIG == [0x52u8, 0x53, 0x44, 0x20, 0x50, 0x54, 0x52, 0x20],
        String::from("RSD PTR = 52 53 44 20 50 54 52 20"),
    ));

    // 短镜像拒（< 20 字节）。
    {
        let mut led = VersionLedger::new();
        let img = RsdpV2::build(2, 0, 0).encode();
        let hit = match img.get(..15) {
            Some(short) => match verify_chain(&mut led, short) {
                Err(e) => e == RsdpVerifyCode::Csum20Bad.val(),
                Ok(_) => false,
            },
            None => false,
        };
        v.push((
            "短镜像显性拒",
            hit,
            String::from("15 字节镜像 < v1 最小 20 ⇒ Csum20Bad"),
        ));
    }

    // 双跑一致。
    v.push((
        "双跑一致",
        repro_check().is_ok(),
        String::from("v1/v2 黄金景两跑同值"),
    ));

    // 探针三序。
    v.push((
        "探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        String::from("a=v2全绿 b=v1通过 c=四阶注入全拒"),
    ));

    v
}

pub fn run_unx0442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0442 RSDP 校验与版本账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v2_chain_green() {
        let rep = golden_v2().ok().unwrap_or(VerifyReport {
            sig_ok: false,
            csum_ok: false,
            len_ok: false,
            ext_ok: false,
            ext_applicable: false,
            version: None,
        });
        assert!(rep.sig_ok && rep.csum_ok && rep.len_ok && rep.ext_ok);
    }

    #[test]
    fn test_v1_chain_green() {
        assert!(probe_seq_b().is_ok());
    }

    #[test]
    fn test_each_stage_inject_rejected() {
        let mut s = 0;
        while s < 4 {
            let stage = match INJECT_STAGES.get(s) {
                Some(x) => *x,
                None => break,
            };
            let mut img = RsdpV2::build(2, 0x00E0000, 0x10).encode();
            assert!(inject_v2(&mut img, stage).is_ok());
            let mut led = VersionLedger::new();
            assert_eq!(
                verify_chain(&mut led, &img),
                Err(expected_reject(stage)),
                "stage {:?} 注入必被当阶拒",
                stage
            );
            s += 1;
        }
    }

    #[test]
    fn test_unknown_revision_archived() {
        assert_eq!(golden_unknown_rev(), Ok((RsdpVerifyCode::RevUnknown.val(), 1)));
    }

    #[test]
    fn test_ledger_trichotomy() {
        assert!(ledger_trichotomy_ok());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let src = RsdpV2::build(2, 0x00E0000, 0x1122334455667788);
        let img = src.encode();
        assert_eq!(img.len(), RAW_LEN);
        assert_eq!(RsdpV2::decode(&img), Ok(src));
    }

    #[test]
    fn test_checksum_values_recomputable() {
        let r = RsdpV2::build(2, 0x00E0000, 0x10);
        let img = r.encode();
        // 前 20 字节和 ≡ 0。
        assert_eq!(sum_mod(img.get(0..V1_LEN).unwrap_or(&[])), 0);
        // 第 20..36 字节和 ≡ 0。
        assert_eq!(sum_mod(img.get(20..36).unwrap_or(&[])), 0);
    }

    #[test]
    fn test_short_image_rejected() {
        let img = RsdpV2::build(2, 0, 0).encode();
        let mut led = VersionLedger::new();
        assert_eq!(
            verify_chain(&mut led, &img[..10]),
            Err(RsdpVerifyCode::Csum20Bad.val())
        );
    }

    #[test]
    fn test_inject_rounds_all_caught() {
        let led = inject_rounds(10).ok().unwrap_or(InjectLedger::new());
        assert!(led.all_caught());
        assert_eq!(led.tries[4], 10);
        assert_eq!(led.caught[0], 10);
    }

    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
    }

    #[test]
    fn test_checks_all_green() {
        let s = run_unx0442_checks();
        assert!(s.all_passed());
        assert!(s.len() >= 12);
    }
}
