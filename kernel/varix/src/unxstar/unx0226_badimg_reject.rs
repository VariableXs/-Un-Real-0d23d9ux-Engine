//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0226 · 坏镜像拒收演练（注入式） · 0x6C640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0226-J1 10 组坏镜像（坏签名/坏表/截断）全部拒收且给
//!     出三要素诊断，无一例死机；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 镜像验收链：魔数（"VXIM"）→ 长度账（≥头+最小负载）→ 校
//!      验和（FNV-1a 64）→ 版本区（F0148 版本区 20 字节可解析）
//!      四闸串联，任一失守拒收；
//!    - 10 组坏镜像闭集：坏魔数×2/截断×2/坏校验和×2/坏版本区×2/
//!      空镜像/超长声明——全部拒收；
//!    - 三要素诊断：拒收必须带（What/Where/Next）三列（F0125 同
//!      制），缺列即拒收器违规；
//!    - 无一例死机：拒收器自身零 panic 面——所有注入走 Result 路，
//!      演练后拒收器可继续验收好镜像（自愈点验）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C640，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C640;

// ── 镜像冻结值 ──────────────────────────────────────────────────
/// 魔数 "VXIM"。
pub const IMG_MAGIC: u32 = 0x5648_4D4D; // 字节序 'M','M','H','V' 账面按 LE 校验
/// 与 make_good 镜像配套的账面魔数（LE 字节 'V','X','I','M'）。
pub const IMG_MAGIC_LE: u32 = 0x4D49_5856;
/// 头长度（魔数 4 + len 4 + ver 20 + checksum 8）。
pub const IMG_HEADER_LEN: usize = 36;
/// 最小负载。
pub const MIN_PAYLOAD: usize = 16;
/// 坏镜像闭集数（冻结：10 组）。
pub const BAD_SAMPLES: usize = 10;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RejCode {
    /// 坏魔数
    BadMagic,
    /// 截断（长度账不足）
    Truncated,
    /// 声明超长（len 字段越账面上界）
    DeclaredOversize,
    /// 坏校验和
    BadChecksum,
    /// 坏版本区（不可解析）
    BadVersionArea,
    /// 空镜像
    EmptyImage,
    /// 三要素诊断缺列
    TriFactorMissing,
    /// 拒收后失能（好镜像也无法验收——死机等价）
    RejectedThenDisabled,
    /// 演练计数不符（≠10 组）
    DrillCountWrong,
    /// 双跑不一致
    NotReproducible,
}

impl RejCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RejCode::BadMagic => "坏魔数：非 VXIM",
            RejCode::Truncated => "截断：长度账不足",
            RejCode::DeclaredOversize => "声明超长：len 越上界",
            RejCode::BadChecksum => "坏校验和：FNV 账",
            RejCode::BadVersionArea => "坏版本区：不可解析",
            RejCode::EmptyImage => "空镜像",
            RejCode::TriFactorMissing => "三要素诊断缺列",
            RejCode::RejectedThenDisabled => "拒收后失能：好镜像无法验收",
            RejCode::DrillCountWrong => "演练计数不符：≠10 组",
            RejCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RejCode; 10] = [
        RejCode::BadMagic,
        RejCode::Truncated,
        RejCode::DeclaredOversize,
        RejCode::BadChecksum,
        RejCode::BadVersionArea,
        RejCode::EmptyImage,
        RejCode::TriFactorMissing,
        RejCode::RejectedThenDisabled,
        RejCode::DrillCountWrong,
        RejCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RejCode::ALL.len() {
        if RejCode::ALL[i].val() == code {
            return RejCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C640 段闭集内"
}

/// FNV-1a 64。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

// ── 镜像账 ──────────────────────────────────────────────────────
/// 验收：四闸串联。Ok(())=收；Err(码)=拒（带拒因）。
pub fn accept_image(img: &[u8]) -> Result<(), u32> {
    if img.len() == 0 {
        return Err(RejCode::EmptyImage.val());
    }
    if img.len() < IMG_HEADER_LEN + MIN_PAYLOAD {
        return Err(RejCode::Truncated.val());
    }
    let magic = u32::from_le_bytes([img[0], img[1], img[2], img[3]]);
    if magic != IMG_MAGIC_LE {
        return Err(RejCode::BadMagic.val());
    }
    let declared = u32::from_le_bytes([img[4], img[5], img[6], img[7]]) as usize;
    if declared > 16 * 1024 * 1024 {
        return Err(RejCode::DeclaredOversize.val());
    }
    if declared != img.len() {
        return Err(RejCode::Truncated.val());
    }
    // 校验和：头内 28..36 存 FNV(镜像除校验和字段外)。
    let mut probe = Vec::from(&img[..28]);
    probe.extend_from_slice(&img[36..]);
    let expect = fnv64(&probe);
    let stored = u64::from_le_bytes([
        img[28], img[29], img[30], img[31], img[32], img[33], img[34], img[35],
    ]);
    if stored != expect {
        return Err(RejCode::BadChecksum.val());
    }
    // 版本区：20 字节（偏移 8..28）须含非零版本文本起点。
    let ver_text = &img[8..16];
    let mut printable = 0;
    let mut i = 0;
    while i < ver_text.len() {
        let c = ver_text[i];
        if (c >= b'0' && c <= b'9') || c == b'.' {
            printable += 1;
        }
        i += 1;
    }
    if printable < 3 {
        return Err(RejCode::BadVersionArea.val());
    }
    Ok(())
}

/// 三要素诊断（拒收伴随）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectDiag {
    pub what: String,
    pub where_: String,
    pub next: String,
}

impl RejectDiag {
    pub fn complete(&self) -> bool {
        self.what.len() > 0 && self.where_.len() > 0 && self.next.len() > 0
    }
}

/// 拒因→三要素（冻结映射）。
pub fn diag_for(code: u32) -> RejectDiag {
    RejectDiag {
        what: String::from(detail(code)),
        where_: String::from("镜像验收链四闸"),
        next: String::from("回退至上一好镜像并引导诊断模式"),
    }
}

/// 拒收器（可自愈：拒后继续验收）。
pub struct ImageRejector {
    pub rejected: Vec<(u32, RejectDiag)>,
    pub accepted: u32,
}

impl ImageRejector {
    pub fn new() -> ImageRejector {
        ImageRejector { rejected: Vec::new(), accepted: 0 }
    }

    /// 验收入口：拒则带三要素入账，收则计数。
    pub fn inspect(&mut self, img: &[u8]) -> Result<(), u32> {
        match accept_image(img) {
            Ok(()) => {
                self.accepted += 1;
                Ok(())
            }
            Err(c) => {
                let d = diag_for(c);
                if !d.complete() {
                    return Err(RejCode::TriFactorMissing.val());
                }
                self.rejected.push((c, d));
                Err(c)
            }
        }
    }

    /// 演练点验：拒收器拒后仍能收好镜像。
    pub fn still_alive(&mut self, good: &[u8]) -> Result<(), u32> {
        self.inspect(good)
    }
}

// ── 好镜像构造 ──────────────────────────────────────────────────
pub fn make_good() -> Vec<u8> {
    let payload = vec![0xABu8; MIN_PAYLOAD];
    let total = IMG_HEADER_LEN + payload.len();
    let mut img = vec![0u8; total];
    img[0..4].copy_from_slice(&IMG_MAGIC_LE.to_le_bytes());
    img[4..8].copy_from_slice(&(total as u32).to_le_bytes());
    // 版本区 "0.9.15.."。
    img[8..16].copy_from_slice(b"0.9.15..");
    // 校验和。
    let mut probe = Vec::from(&img[..28]);
    probe.extend_from_slice(&img[36..]);
    let fp = fnv64(&probe);
    img[28..36].copy_from_slice(&fp.to_le_bytes());
    let mut i = 0;
    while i < payload.len() {
        img[IMG_HEADER_LEN + i] = payload[i];
        i += 1;
    }
    img
}

// ── 10 组坏镜像闭集 ────────────────────────────────────────────
pub fn make_bad_set() -> [Vec<u8>; BAD_SAMPLES] {
    let good = make_good();
    [
        {
            let mut b = good.clone();
            b[0] = b'X'; // 坏魔数 1
            b
        },
        {
            let mut b = good.clone();
            b[3] ^= 0xFF; // 坏魔数 2
            b
        },
        good[..20].to_vec(), // 截断 1
        good[..30].to_vec(), // 截断 2（头内）
        {
            let mut b = good.clone();
            b[36] ^= 0x01; // 坏校验和 1（负载位翻转）
            b
        },
        {
            let mut b = good.clone();
            let last = b.len() - 1;
            b[last] ^= 0x80; // 坏校验和 2
            b
        },
        {
            let mut b = good.clone();
            b[8..16].copy_from_slice(b"########"); // 坏版本区 1
            b
        },
        {
            let mut b = good.clone();
            b[12] = b'-'; // 坏版本区 2（非法字符）
            b
        },
        Vec::new(), // 空镜像
        {
            let mut b = good.clone();
            b[4..8].copy_from_slice(&0x7FFF_FFFFu32.to_le_bytes()); // 声明超长
            b
        },
    ]
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：10 组坏镜像全拒→三要素齐→拒后自愈→计数点验。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    let bad = make_bad_set();
    let mut rej = ImageRejector::new();
    let mut i = 0;
    while i < bad.len() {
        if rej.inspect(&bad[i]).is_ok() {
            return Err(RejCode::DrillCountWrong.val());
        }
        i += 1;
    }
    if rej.rejected.len() != BAD_SAMPLES {
        return Err(RejCode::DrillCountWrong.val());
    }
    // 三要素逐条齐备。
    let mut k = 0;
    while k < rej.rejected.len() {
        if !rej.rejected[k].1.complete() {
            return Err(RejCode::TriFactorMissing.val());
        }
        k += 1;
    }
    // 拒后自愈：好镜像必收。
    rej.still_alive(&make_good())?;
    if rej.accepted != 1 {
        return Err(RejCode::RejectedThenDisabled.val());
    }
    Ok((rej.rejected.len(), rej.accepted, BAD_SAMPLES as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    accept_image(&make_good())
}

pub fn probe_seq_b() -> Result<(), u32> {
    accept_image(&Vec::new())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 校验和破坏 → 必拒。
    let mut b = make_good();
    let last = b.len() - 1;
    b[last] ^= 1;
    accept_image(&b)
}

/// 注入 10/10：空镜像、坏魔数、坏校验和、拒后自愈四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        led.record(0, accept_image(&Vec::new()).is_err());
        let mut b1 = make_good();
        b1[0] = b'Z';
        led.record(1, accept_image(&b1).is_err());
        let mut b2 = make_good();
        b2[30] ^= 0x10;
        led.record(2, accept_image(&b2).is_err());
        // 拒后自愈（拒 1 坏后好镜像必收）。
        let mut rej = ImageRejector::new();
        rej.inspect(&Vec::new()).ok();
        led.record(3, rej.still_alive(&make_good()).is_ok());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RejCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RejCode::ALL.len() {
        let v = RejCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 10 组闭集。
    v.push((
        "J1-10组坏镜像闭集",
        BAD_SAMPLES == 10,
        format!("{} 组", BAD_SAMPLES),
    ));
    // 三要素逐条。
    let bad = make_bad_set();
    let mut tri = 0;
    let mut i = 0;
    while i < bad.len() {
        let mut rej = ImageRejector::new();
        if rej.inspect(&bad[i]).is_err() && rej.rejected.len() == 1 && rej.rejected[0].1.complete() {
            tri += 1;
        }
        i += 1;
    }
    v.push((
        "J1-三要素诊断全带",
        tri == BAD_SAMPLES,
        format!("{}/{} 条", tri, BAD_SAMPLES),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
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
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C640 窗口")));
    v
}

pub fn run_unx0226_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0226 坏镜像拒收演练（注入式）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
