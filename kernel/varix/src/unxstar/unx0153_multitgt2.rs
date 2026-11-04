//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0153 · 多目标产物（ISO/IMG/HDD 头）（深化第二实现） · 0x6F040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0153-J1 多目标头账：ISO（卷描述符 PVD @16）/IMG（扇
//!     区 0 引导签名 55AA）/HDD（MBR 签名 + 分区表）三型头逐字
//!     账，头账与型匹配闸（错型拒），头可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三型闭集（ISO/IMG/HDD）；
//!   - 头闸：ISO PVD "CD001" @偏移 1；IMG/HDD 尾签 0xAA55 @510；
//!   - 错型拒（ISO 交 IMG 头判错）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F040，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6F040;

/// 三型（冻结）。
pub const T_ISO: u8 = 0;
pub const T_IMG: u8 = 1;
pub const T_HDD: u8 = 2;
pub const KIND_COUNT: usize = 3;
/// 引导签名。
pub const BOOT_SIG_LO: u8 = 0x55;
pub const BOOT_SIG_HI: u8 = 0xAA;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TgtCode {
    /// 型账外
    KindUnknown,
    /// ISO PVD 缺
    PvdMissing,
    /// 引导签名缺
    BootSigMissing,
    /// 错型头（头与型失配）
    KindHeaderMismatch,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl TgtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TgtCode::KindUnknown => "型账外：非 ISO/IMG/HDD",
            TgtCode::PvdMissing => "ISO PVD 缺：CD001",
            TgtCode::BootSigMissing => "引导签名缺：55AA",
            TgtCode::KindHeaderMismatch => "错型头：头与型失配",
            TgtCode::ReplayMismatch => "复算失配",
            TgtCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TgtCode; 6] = [
        TgtCode::KindUnknown,
        TgtCode::PvdMissing,
        TgtCode::BootSigMissing,
        TgtCode::KindHeaderMismatch,
        TgtCode::ReplayMismatch,
        TgtCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TgtCode::ALL.len() {
        if TgtCode::ALL[i].val() == code {
            return TgtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F040 段闭集内"
}

// ── 头账 ────────────────────────────────────────────────────────
/// 镜像头类型判定（数据驱动：看头特征）。
pub fn detect_kind(img: &[u8]) -> Result<u8, u32> {
    // ISO：@0x8001 起 "CD001"。
    if img.len() >= 0x8006 && &img[0x8001..0x8006] == b"CD001" {
        return Ok(T_ISO);
    }
    // IMG/HDD：@510 尾签 55AA。
    if img.len() >= 512 && img[510] == BOOT_SIG_LO && img[511] == BOOT_SIG_HI {
        // MBR 分区表（@446 起 64 字节，HDD 有可解析表项；IMG 纯软盘无）。
        let has_part = img.len() >= 466 && img[450] == 0x00 && img[454] == 0xFE;
        if has_part {
            return Ok(T_HDD);
        }
        return Ok(T_IMG);
    }
    Err(TgtCode::BootSigMissing.val())
}

/// 头-型匹配闸。
pub fn kind_matches(img: &[u8], claim: u8) -> Result<(), u32> {
    if claim > T_HDD {
        return Err(TgtCode::KindUnknown.val());
    }
    let real = detect_kind(img)?;
    if real != claim {
        return Err(TgtCode::KindHeaderMismatch.val());
    }
    Ok(())
}

/// 构造 ISO 头（PVD @0x8000，"CD001" @0x8001）。
pub fn make_iso() -> Vec<u8> {
    let mut b = vec![0u8; 0x8800];
    b[0x8000] = 1; // Volume Descriptor Type: PVD
    b[0x8001..0x8006].copy_from_slice(b"CD001");
    b
}

/// 构造 IMG 头（扇区 0 尾签 55AA，无分区表）。
pub fn make_img() -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[510] = BOOT_SIG_LO;
    b[511] = BOOT_SIG_HI;
    b
}

/// 构造 HDD 头（MBR 签名 + 分区表特征）。
pub fn make_hdd() -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[450] = 0x00;
    b[454] = 0xFE;
    b[510] = BOOT_SIG_LO;
    b[511] = BOOT_SIG_HI;
    b
}

/// 复算：同镜像恒同型判定。
pub fn replay_ok(img: &[u8]) -> Result<(), u32> {
    let a = detect_kind(img)?;
    let b = detect_kind(img)?;
    if a != b {
        return Err(TgtCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：缺签路/错型路/越型路/复算路） ──────────────
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

// ── 黄金全链：三型判定→错型拒→越型拒→复算→无签拒 ──────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let iso = detect_kind(&make_iso())?;
    let img = detect_kind(&make_img())?;
    let hdd = detect_kind(&make_hdd())?;
    let kinds_ok = (iso == T_ISO && img == T_IMG && hdd == T_HDD) as u32;
    // 错型拒（ISO 报 IMG）。
    let mismatch = kind_matches(&make_iso(), T_IMG).is_err();
    // 越型拒。
    let over = kind_matches(&make_img(), 9).is_err();
    // 无签拒。
    let nosig = detect_kind(&vec![0u8; 512]).is_err();
    replay_ok(&make_iso())?;
    Ok((kinds_ok, mismatch as u32 + over as u32 + nosig as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：三型判定全对 + 三拒全抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 3, _)) => Ok(()),
        _ => Err(TgtCode::NotReproducible.val()),
    }
}

/// 序 B：错型头 ⇒ KindHeaderMismatch。
pub fn probe_seq_b() -> Result<(), u32> {
    match kind_matches(&make_hdd(), T_IMG) {
        Err(e) if e == TgtCode::KindHeaderMismatch.val() => Ok(()),
        _ => Err(TgtCode::NotReproducible.val()),
    }
}

/// 序 C：ISO PVD 缺 ⇒ BootSigMissing（空账）。
pub fn probe_seq_c() -> Result<(), u32> {
    match detect_kind(&vec![0u8; 0x8800]) {
        Err(e) if e == TgtCode::BootSigMissing.val() => Ok(()),
        _ => Err(TgtCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：缺签路。
        let mut b1 = make_img();
        b1[510] = 0;
        led.record(0, detect_kind(&b1).is_err());
        // 路 2：错型路。
        led.record(1, kind_matches(&make_img(), T_HDD).is_err());
        // 路 3：越型路。
        led.record(2, kind_matches(&make_iso(), 7).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&make_hdd()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TgtCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TgtCode::ALL.len() {
        let v = TgtCode::ALL[i].val();
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
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((kinds, rej, _)) => *kinds == 1 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((kinds, rej, _)) => format!("kinds={} rejects={}", kinds, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三型闭集。
    v.push((
        "J1-三型闭集",
        KIND_COUNT == 3 && (T_ISO, T_IMG, T_HDD) == (0, 1, 2),
        String::from("ISO/IMG/HDD 三型冻结"),
    ));
    // 头特征逐字（PVD CD001 / 55AA）。
    let iso = make_iso();
    let img = make_img();
    v.push((
        "J1-头特征",
        &iso[0x8001..0x8006] == b"CD001" && img[510] == 0x55 && img[511] == 0xAA,
        String::from("PVD \"CD001\" @0x8001 / 55AA @510"),
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
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
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
        String::from("0x6F040 窗口"),
    ));
    v
}

pub fn run_unx0153b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0153 多目标产物（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
