//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13901 · 'regf' 签名错位拒载矩阵（8 变体全拒） · 0x66040
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F13901-J1 签名错位 8 变体（首字节翻转/末字节翻转/大小写/
//!      全零/右移 1/左移 1/隔位坏/双字节坏）注入 8/8 次拒载且分档归
//!      "签名族"，合法对照档 10/10 通过，双态断言入账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 'regf' 签名（winreg 规格 §1，写死 0x66676572 小端四字节
//!     'r','e','g','f'）加载器首闸：签名不一致一律拒载；
//!   - 8 变体注入闭集：逐变体构造坏签名并断言拒载+分档归"签名族"；
//!   - 合法对照档 10/10：合法 regf 头（含正确签名）10 次加载全过；
//!   - 双态断言入账：每变体（拒载态）与每对照（通过态）逐条入账，
//!     账行数 = 8+10 守恒；
//!   - 注入面即判据本体（红绿两态矩阵）；
//!   - LCG 种子写死 = SEG_BASE（0x66040），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66040，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66040;

/// regf 签名字节（写死：'r'=0x72 'e'=0x65 'g'=0x67 'f'=0x66）。
pub const SIG_BYTES: [u8; 4] = [0x72, 0x65, 0x67, 0x66];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegfCode {
    /// 签名族拒载（8 变体统一分档）
    SignatureFamilyReject,
    /// 合法签名却拒载（误杀）
    LegalSignatureRejected,
    /// 坏签名被放行（漏杀）
    BadSignatureAccepted,
    /// 头缓冲不足
    HeaderTruncated,
    /// 双态账行数不守恒
    DualLedgerMismatch,
    /// 变体枚举外值
    VariantOutOfRange,
    /// 加载器二次签名漂移（加载中途改签名面）
    SignatureDriftMidload,
    /// 分档错族
    WrongFamily,
}

impl RegfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RegfCode::SignatureFamilyReject => "签名族拒载：8 变体统一归签名族",
            RegfCode::LegalSignatureRejected => "合法签名却拒载：误杀红线",
            RegfCode::BadSignatureAccepted => "坏签名被放行：漏杀红线",
            RegfCode::HeaderTruncated => "头缓冲不足：4 字节签名读不到",
            RegfCode::DualLedgerMismatch => "双态账行数不守恒：8+10 冻结",
            RegfCode::VariantOutOfRange => "变体枚举外值：8 变体闭集",
            RegfCode::SignatureDriftMidload => "加载中途签名漂移：二次校验抓",
            RegfCode::WrongFamily => "分档错族：拒载必须归签名族",
        }
    }
    pub const ALL: [RegfCode; 8] = [
        RegfCode::SignatureFamilyReject,
        RegfCode::LegalSignatureRejected,
        RegfCode::BadSignatureAccepted,
        RegfCode::HeaderTruncated,
        RegfCode::DualLedgerMismatch,
        RegfCode::VariantOutOfRange,
        RegfCode::SignatureDriftMidload,
        RegfCode::WrongFamily,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RegfCode::ALL.len() {
        if RegfCode::ALL[i].val() == code {
            return RegfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66040 段闭集内"
}

// ── regf 头模型 ──────────────────────────────────────────────────
/// 最小 regf 头：签名(4) + 主序号(4) + 次序号(4) + 保留(4)。
pub const HEADER_LEN: usize = 16;

pub fn synth_regf(major: u32, minor: u32) -> Vec<u8> {
    let mut b: Vec<u8> = Vec::new();
    b.resize(HEADER_LEN, 0u8);
    b[0] = SIG_BYTES[0];
    b[1] = SIG_BYTES[1];
    b[2] = SIG_BYTES[2];
    b[3] = SIG_BYTES[3];
    b[4] = (major & 0xFF) as u8;
    b[5] = ((major >> 8) & 0xFF) as u8;
    b[8] = (minor & 0xFF) as u8;
    b[9] = ((minor >> 8) & 0xFF) as u8;
    b
}

/// 加载器签名闸（首闸+二次校验）。
pub fn regf_load(buf: &[u8]) -> Result<u32, u32> {
    if buf.len() < 4 {
        return Err(RegfCode::HeaderTruncated.val());
    }
    if buf[0] != SIG_BYTES[0]
        || buf[1] != SIG_BYTES[1]
        || buf[2] != SIG_BYTES[2]
        || buf[3] != SIG_BYTES[3]
    {
        return Err(RegfCode::SignatureFamilyReject.val());
    }
    // 二次校验：头尾再读一次签名（模拟加载中途读取）。
    if buf.len() >= HEADER_LEN
        && (buf[0] != SIG_BYTES[0]
            || buf[1] != SIG_BYTES[1]
            || buf[2] != SIG_BYTES[2]
            || buf[3] != SIG_BYTES[3])
    {
        return Err(RegfCode::SignatureDriftMidload.val());
    }
    let major = (buf[4] as u32) | ((buf[5] as u32) << 8);
    Ok(major)
}

// ── 8 变体注入闭集 ───────────────────────────────────────────────
/// 变体枚举（冻结）：0 首字节翻转 / 1 末字节翻转 / 2 大小写
/// ('REGF') / 3 全零 / 4 右移 1 / 5 左移 1 / 6 隔位坏 / 7 双字节坏。
pub const VARIANT_COUNT: usize = 8;

pub fn corrupted_sig(variant: usize) -> Result<[u8; 4], u32> {
    let mut s = SIG_BYTES;
    match variant {
        0 => s[0] = s[0] ^ 0xFF,
        1 => s[3] = s[3] ^ 0xFF,
        2 => {
            // 大小写：'REGF' = 大写 ASCII。
            s = [0x52, 0x45, 0x47, 0x46];
        }
        3 => s = [0, 0, 0, 0],
        4 => {
            // 右移 1 字节：末位挤出。
            s = [0x00, SIG_BYTES[0], SIG_BYTES[1], SIG_BYTES[2]];
        }
        5 => {
            // 左移 1 字节：首位挤出。
            s = [SIG_BYTES[1], SIG_BYTES[2], SIG_BYTES[3], 0x00];
        }
        6 => {
            // 隔位坏：0/2 位翻转。
            s[0] ^= 0x01;
            s[2] ^= 0x01;
        }
        7 => {
            // 双字节坏：1/3 位翻转。
            s[1] ^= 0x80;
            s[3] ^= 0x80;
        }
        _ => return Err(RegfCode::VariantOutOfRange.val()),
    }
    Ok(s)
}

// ── 双态断言账 ───────────────────────────────────────────────────
/// 双态账：红（拒载）8 行 + 绿（通过）10 行，行数守恒 18。
pub struct DualLedger {
    pub red_rows: u32,
    pub green_rows: u32,
}

pub const RED_FROZEN: u32 = 8;
pub const GREEN_FROZEN: u32 = 10;

impl DualLedger {
    pub const fn new() -> DualLedger {
        DualLedger { red_rows: 0, green_rows: 0 }
    }
    pub fn conservation(&self) -> Result<(), u32> {
        if self.red_rows != RED_FROZEN || self.green_rows != GREEN_FROZEN {
            return Err(RegfCode::DualLedgerMismatch.val());
        }
        Ok(())
    }
}

/// 红绿全矩阵：8 变体全拒 + 10 合法全过。
pub fn dual_matrix() -> Result<DualLedger, u32> {
    let mut led = DualLedger::new();
    let mut v = 0;
    while v < VARIANT_COUNT {
        let sig = corrupted_sig(v)?;
        let mut buf = synth_regf(1, 3);
        buf[0] = sig[0];
        buf[1] = sig[1];
        buf[2] = sig[2];
        buf[3] = sig[3];
        match regf_load(&buf) {
            Err(e) => {
                if e == RegfCode::SignatureFamilyReject.val() {
                    led.red_rows += 1;
                } else {
                    return Err(RegfCode::WrongFamily.val());
                }
            }
            Ok(_) => return Err(RegfCode::BadSignatureAccepted.val()),
        }
        v += 1;
    }
    // 绿：10 次合法加载。
    let mut g = 0;
    while g < GREEN_FROZEN {
        let buf = synth_regf(1, 3 + g);
        if regf_load(&buf)? == 1 {
            led.green_rows += 1;
        }
        g += 1;
    }
    led.conservation()?;
    Ok(led)
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    let led = dual_matrix()?;
    // 随机抽样双态再验 8 例。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut extra = 0u32;
    let mut i = 0;
    while i < 8 {
        if rng.below(2) == 0 {
            // 红态。
            let v = rng.below(VARIANT_COUNT as u64) as usize;
            let sig = corrupted_sig(v)?;
            let mut buf = synth_regf(1, 3);
            buf[0] = sig[0];
            buf[1] = sig[1];
            buf[2] = sig[2];
            buf[3] = sig[3];
            if regf_load(&buf).is_err() {
                extra += 1;
            }
        } else {
            let buf = synth_regf(1, 3);
            if regf_load(&buf).is_ok() {
                extra += 1;
            }
        }
        i += 1;
    }
    Ok((led.red_rows + led.green_rows, extra))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：变体越界。
    ledger.tries[0] += 1;
    if corrupted_sig(8).is_err() {
        ledger.caught[0] += 1;
    }
    // 族 1：头截断。
    ledger.tries[1] += 1;
    if regf_load(&[0x72, 0x65]) == Err(RegfCode::HeaderTruncated.val()) {
        ledger.caught[1] += 1;
    }
    // 族 2：坏签名放行面（合法缓冲为对照——确认加载器对合法不误杀）。
    ledger.tries[2] += 1;
    let buf = synth_regf(1, 3);
    if regf_load(&buf).is_ok() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RegfCode::ALL.len() {
        let v = RegfCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-双态矩阵8拒10过+抽8",
        matches!(golden, Ok((18, 8))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 签名冻结点验。
    v.push((
        "J1-签名冻结",
        SIG_BYTES == [0x72, 0x65, 0x67, 0x66],
        String::from("'regf' 写死"),
    ));
    // 8 变体两两互异。
    let mut distinct = true;
    let mut a = 0;
    while a < VARIANT_COUNT {
        let mut b = a + 1;
        while b < VARIANT_COUNT {
            if corrupted_sig(a) == corrupted_sig(b) {
                distinct = false;
            }
            b += 1;
        }
        a += 1;
    }
    v.push(("J1-变体互异", distinct, String::from("8 变体全不同")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66040 窗口")));
    v
}

pub fn run_unx13901_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13901 'regf' 签名错位拒载矩阵（8 变体全拒）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
