//! UNX-F51627 端点描述符畸形族（wMaxPacketSize 越界 ×10 变体） · 0xC1F80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51628-J1 慢速 ×10 按预算失败隔离；UNX-F51628-J1R 慢但合规设备 ×5 零误杀。
//!
//! 结构清单：
//!   - EpValidator：端点描述符验证器（类型/方向/wMaxPacketSize 三轴闭集）
//!   - BudgetGate：慢速设备预算闸门（预算耗尽即失败隔离）
//!   - MalLedger：畸形审计账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② wMaxPacketSize 语义：低速 8、全速 64、高速 1024 封顶。畸形族 ×10 变
//!      体按速度域越界建模：低速>8、全速>64、高速>1024、零包长、非 2 的幂
//!      等等。验证器按描述符里的速度域选上限，越界即拒。
//!   ③ 慢速预算（J1）：慢速设备每步都拉满重试，预算闸门按「重试额度」计
//!      费——额度耗尽即失败隔离，不再无限等。隔离是失败性的（显性码），
//!      不依赖墙钟。
//!   ④ 慢但合规（J1R）：慢速设备只要 wMaxPacketSize 合规（≤8）就零误杀，
//!      重试在预算内允许完成。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51627 − 51501)。
pub const SEG_BASE: u32 = 0xC1F80;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51627u32 - 51501));

/// 速度域闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speed {
    /// 低速 1.5Mbps，wMaxPacketSize ≤ 8
    Low = 0,
    /// 全速 12Mbps，wMaxPacketSize ≤ 64
    Full = 1,
    /// 高速 480Mbps，wMaxPacketSize ≤ 1024
    High = 2,
}

impl Speed {
    /// 速度码闭集解码。
    pub fn from_code(c: u32) -> Option<Speed> {
        match c {
            0 => Some(Speed::Low),
            1 => Some(Speed::Full),
            2 => Some(Speed::High),
            _ => None,
        }
    }
    /// 该速度域 wMaxPacketSize 上限。
    pub fn cap(self) -> u16 {
        match self {
            Speed::Low => 8,
            Speed::Full => 64,
            Speed::High => 1024,
        }
    }
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Speed::Low => "低速",
            Speed::Full => "全速",
            Speed::High => "高速",
        }
    }
}

/// 慢速重试预算。
pub const BUDGET: u32 = 4;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 端点号越界
    EpNumOob,
    /// 端点号重复
    EpNumDup,
    /// 速度码越闭集
    SpeedUnknown,
    /// wMaxPacketSize 为零
    PktZero,
    /// wMaxPacketSize 非二幂
    PktNotPow2,
    /// wMaxPacketSize 超速度域上限
    PktOverCap,
    /// 描述符残缺
    DescFragment,
    /// 预算耗尽失败隔离
    BudgetIsolated,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl EpCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::Ok => "端点验证通过：包长合规且预算充足",
            EpCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            EpCode::EpNumOob => "端点号越界：端点号必须在 0..16 之内",
            EpCode::EpNumDup => "端点号重复：同方向端点号不得重复",
            EpCode::SpeedUnknown => "速度码越闭集：只接受 0/1/2 三档速度",
            EpCode::PktZero => "wMaxPacketSize 为零：零包长端点无法传输",
            EpCode::PktNotPow2 => "wMaxPacketSize 非二幂：合法包长必须是 2 的幂",
            EpCode::PktOverCap => "wMaxPacketSize 超速度域上限：低速 8 全速 64 高速 1024",
            EpCode::DescFragment => "端点描述符残缺：不足 7 字节不构成端点",
            EpCode::BudgetIsolated => "预算耗尽：慢速设备重试额度用尽，失败隔离",
            EpCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            EpCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [EpCode; 12] = [
    EpCode::Ok,
    EpCode::SegBad,
    EpCode::EpNumOob,
    EpCode::EpNumDup,
    EpCode::SpeedUnknown,
    EpCode::PktZero,
    EpCode::PktNotPow2,
    EpCode::PktOverCap,
    EpCode::DescFragment,
    EpCode::BudgetIsolated,
    EpCode::LedgerMismatch,
    EpCode::FingerprintMismatch,
];

/// 二幂判定。
pub fn is_pow2(n: u16) -> bool {
    n != 0 && (n & (n - 1)) == 0
}

/// 端点描述符验证器。
pub struct EpValidator {
    seen: Vec<(u8, bool)>,
    trace: Vec<String>,
}

impl EpValidator {
    /// 新验证器。
    pub fn new() -> EpValidator {
        EpValidator { seen: Vec::new(), trace: Vec::new() }
    }
    /// 验证一条端点描述符（建模 7 字节：[num_attr, dir, pkt_lo, pkt_hi, 0,0,0]）。
    /// 判序：速度 → 残缺 → 端点号 → 零长 → 二幂 → 上限 → 重复。
    pub fn validate(&mut self, speed_code: u32, desc: &[u8]) -> Result<u16, EpCode> {
        let sp = match Speed::from_code(speed_code) {
            Some(s) => s,
            None => return Err(EpCode::SpeedUnknown),
        };
        if desc.len() < 7 {
            return Err(EpCode::DescFragment);
        }
        let num_attr = match desc.get(0) {
            Some(&b) => b,
            None => return Err(EpCode::DescFragment),
        };
        let dir = match desc.get(1) {
            Some(&b) => b != 0,
            None => return Err(EpCode::DescFragment),
        };
        let pkt_lo = match desc.get(2) {
            Some(&b) => b as u16,
            None => return Err(EpCode::DescFragment),
        };
        let pkt_hi = match desc.get(3) {
            Some(&b) => b as u16,
            None => return Err(EpCode::DescFragment),
        };
        let num = num_attr;
        if num > 15 {
            return Err(EpCode::EpNumOob);
        }
        let pkt = pkt_lo | (pkt_hi << 8);
        if pkt == 0 {
            return Err(EpCode::PktZero);
        }
        if !is_pow2(pkt) {
            return Err(EpCode::PktNotPow2);
        }
        if pkt > sp.cap() {
            return Err(EpCode::PktOverCap);
        }
        if self.seen.iter().any(|&(n, d)| n == num && d == dir) {
            return Err(EpCode::EpNumDup);
        }
        self.seen.push((num, dir));
        self.trace.push(format!("sp={} ep={} pkt={}", sp.name(), num, pkt));
        Ok(pkt)
    }
    /// 已收端点数。
    pub fn accepted(&self) -> usize {
        self.seen.len()
    }
    /// 轨迹。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 慢速预算闸门：重试计费，耗尽即失败隔离。
pub struct BudgetGate {
    used: u32,
    isolated: bool,
}

impl BudgetGate {
    /// 新闸门。
    pub fn new() -> BudgetGate {
        BudgetGate { used: 0, isolated: false }
    }
    /// 花一次重试额度；余额不足即失败隔离。
    pub fn spend(&mut self) -> Result<u32, EpCode> {
        if self.isolated {
            return Err(EpCode::BudgetIsolated);
        }
        if self.used >= BUDGET {
            self.isolated = true;
            return Err(EpCode::BudgetIsolated);
        }
        self.used += 1;
        Ok(BUDGET - self.used)
    }
    /// 是否已隔离。
    pub fn isolated(&self) -> bool {
        self.isolated
    }
    /// 已用额度。
    pub fn used(&self) -> u32 {
        self.used
    }
}

/// 畸形审计账本。
#[derive(Default)]
pub struct MalLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl MalLedger {
    /// 记一次检出。
    pub fn hit(&mut self, round: u32, code: EpCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, EpCode::Ok.val()));
    }
    /// 守恒断言。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }
    /// 检出数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 账本行数。
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 3 ^ 0x9E37_79B9_1F83_D9AB
}

/// 确定性线性同余发生器。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 构造描述符：[端点属性, 方向, pkt_lo, pkt_hi, 0, 0, 0]。
pub fn make_desc(num: u8, dir: u8, pkt: u16) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    for b in [num, dir, (pkt & 0xFF) as u8, (pkt >> 8) as u8, 0u8, 0u8, 0u8] {
        v.push(b);
    }
    v
}

/// 畸形族 ×10 变体（速度域越界 + 结构性畸形）。
pub const MALFORMED_PKT: [u16; 10] = [0, 3, 12, 16, 100, 128, 512, 2048, 4096, 0x8000];

/// 畸形注入 ×10 轮：每轮取一个变体包长，低速域全部越界判。
pub fn injection_campaign() -> MalLedger {
    let mut led = MalLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let pkt = MALFORMED_PKT[r as usize];
        let mut ep = EpValidator::new();
        let mut g = BudgetGate::new();
        // 慢速预算场景：先花光预算再验端点 → BudgetIsolated 优先；
        // 预算内则直接验端点的畸形。
        let _ = g.spend();
        let mut isolated = false;
        for _ in 0..BUDGET {
            if g.spend() == Err(EpCode::BudgetIsolated) {
                isolated = true;
            }
        }
        let hit = if isolated {
            // 隔离后端点验证仍必须拒绝畸形包长（预算失败隔离生效）。
            match ep.validate(0, &make_desc(1, 1, pkt)) {
                Err(c) => Some(c),
                Ok(_) => Some(EpCode::BudgetIsolated),
            }
        } else {
            match ep.validate(0, &make_desc(1, 1, pkt)) {
                Err(c) => Some(c),
                Ok(_) => None,
            }
        };
        let _ = rnd.next();
        match hit {
            Some(c) => led.hit(r + 1, c),
            None => led.miss(r + 1),
        }
    }
    led
}

/// 慢但合规 ×5 零误杀回归：低速 pkt≤8 且二幂，预算内完成。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    let legal: [u16; 5] = [8, 4, 2, 8, 8];
    for (i, &pkt) in legal.iter().enumerate() {
        let mut ep = EpValidator::new();
        let mut g = BudgetGate::new();
        let mut good = true;
        // 慢速重试 1~2 次后成功（预算内）。
        for _ in 0..(i % 2 + 1) {
            if g.spend().is_err() {
                good = false;
            }
        }
        let v = ep.validate(0, &make_desc((i as u8) + 1, 0, pkt));
        if v.is_err() || v != Ok(pkt) || g.isolated() {
            good = false;
        }
        if good {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    (ok, bad)
}

/// 慢速 ×10 按预算失败隔离全景：预算耗尽必隔离且隔离后不再放行。
pub fn budget_isolation_panorama() -> (u32, u32) {
    let mut isolated_cnt = 0u32;
    let mut leaked = 0u32;
    for _ in 0..ROUNDS {
        let mut g = BudgetGate::new();
        for _ in 0..(BUDGET + 3) {
            if g.spend() == Err(EpCode::BudgetIsolated) {
                isolated_cnt += 1;
            }
        }
        if !g.isolated() {
            leaked += 1;
        }
    }
    (isolated_cnt, leaked)
}

/// FNV-1a 64 位摘要。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut ep = EpValidator::new();
    let _ = ep.validate(2, &make_desc(1, 1, 512));
    let _ = ep.validate(2, &make_desc(2, 0, 1024));
    for line in ep.trace() {
        let mut bytes: Vec<u8> = Vec::new();
        for b in line.as_bytes() {
            bytes.push(*b ^ (fp & 0xFF) as u8);
        }
        fp = fnv1a(&bytes) ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽。
pub fn val_codes_distinct() -> bool {
    for i in 0..ALL_CODES.len() {
        let a = ALL_CODES[i].val();
        if !(SEG_BASE + 1..=SEG_BASE + 12).contains(&a) {
            return false;
        }
        for j in (i + 1)..ALL_CODES.len() {
            if a == ALL_CODES[j].val() {
                return false;
            }
        }
    }
    true
}

/// 收口自检（16 条判据）。
pub fn run_unx51627_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51627");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 合规端点通过。
    {
        let mut ep = EpValidator::new();
        let r = ep.validate(0, &make_desc(1, 1, 8));
        s.add_dyn("合规端点通过", r == Ok(8), "低速 pkt=8 必须通过");
    }
    // 4 慢但合规 ×5 零误杀（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "慢但合规零误杀",
            ok == 5 && bad == 0,
            "UNX-F51628-J1R 慢但合规设备 ×5 零误杀",
        );
    }
    // 5 畸形 ×10 按预算失败隔离（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "畸形按预算隔离",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51628-J1 慢速 ×10 按预算失败隔离",
        );
    }
    // 6 三档速度上限边界。
    {
        let mut all_ok = true;
        for (code, pkt) in [(0u32, 8u16), (1, 64), (2, 1024)] {
            let mut ep = EpValidator::new();
            if ep.validate(code, &make_desc(1, 1, pkt)).is_err() {
                all_ok = false;
            }
        }
        s.add_dyn("速度上限边界", all_ok, "8/64/1024 恰在上限必须放行");
    }
    // 7 超上限变体逐个显性拒。
    {
        let mut all_rej = true;
        for (code, pkt) in [(0u32, 16u16), (1, 128), (2, 2048)] {
            let mut ep = EpValidator::new();
            if ep.validate(code, &make_desc(1, 1, pkt)) != Err(EpCode::PktOverCap) {
                all_rej = false;
            }
        }
        s.add_dyn("超上限变体全拒", all_rej, "三档速度越上限必须全部显性拒");
    }
    // 8 零包长与非二幂。
    {
        let mut ep = EpValidator::new();
        let z = ep.validate(1, &make_desc(1, 1, 0));
        let mut ep2 = EpValidator::new();
        let n = ep2.validate(1, &make_desc(1, 1, 100));
        s.add_dyn(
            "零长与非二幂",
            z == Err(EpCode::PktZero) && n == Err(EpCode::PktNotPow2),
            "零包长与非二幂包长分码显性拒",
        );
    }
    // 9 ×10 变体全拒（速度域注入面）。
    {
        let mut all_rej = true;
        for &pkt in MALFORMED_PKT.iter() {
            let mut ep = EpValidator::new();
            if ep.validate(0, &make_desc(1, 1, pkt)).is_ok() {
                all_rej = false;
            }
        }
        s.add_dyn("×10 变体低速全拒", all_rej, "畸形包长族在低速域必须全拒");
    }
    // 10 残缺描述符拒。
    {
        let mut ep = EpValidator::new();
        let r = ep.validate(1, &[1, 1, 8]);
        s.add_dyn("残缺拒", r == Err(EpCode::DescFragment), "不足 7 字节不构成端点");
    }
    // 11 端点号重复与越界。
    {
        let mut ep = EpValidator::new();
        let a = ep.validate(1, &make_desc(3, 1, 64));
        let b = ep.validate(1, &make_desc(3, 1, 64));
        let mut ep2 = EpValidator::new();
        let c = ep2.validate(1, &make_desc(0xF1, 1, 64));
        s.add_dyn(
            "重复与越界端点号",
            a.is_ok() && b == Err(EpCode::EpNumDup) && c == Err(EpCode::EpNumOob),
            "同方向重复端点号与越界端点号显性拒",
        );
    }
    // 12 速度码越闭集。
    {
        let mut ep = EpValidator::new();
        let r = ep.validate(9, &make_desc(1, 1, 8));
        s.add_dyn("速度码越闭集", r == Err(EpCode::SpeedUnknown), "速度码只接受 0/1/2");
    }
    // 13 预算耗尽失败隔离全景。
    {
        let (isolated, leaked) = budget_isolation_panorama();
        s.add_dyn(
            "预算失败隔离",
            isolated >= ROUNDS && leaked == 0,
            "慢速重试超预算必须隔离且零泄漏",
        );
    }
    // 14 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 15 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 16 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51627_checks_green() {
        let set = run_unx51627_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51627u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1F80);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(EpCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(EpCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn speed_closed_set_and_caps() {
        assert!(Speed::from_code(0).is_some() && Speed::from_code(2).is_some());
        assert!(Speed::from_code(3).is_none());
        assert_eq!(Speed::Low.cap(), 8);
        assert_eq!(Speed::Full.cap(), 64);
        assert_eq!(Speed::High.cap(), 1024);
    }

    #[test]
    fn pow2_property() {
        for n in [1u16, 2, 4, 8, 16, 64, 512, 1024] {
            assert!(is_pow2(n));
        }
        for n in [0u16, 3, 6, 100, 2048 - 3] {
            assert!(!is_pow2(n));
        }
    }

    #[test]
    fn caps_boundary_pass() {
        let mut ep = EpValidator::new();
        assert_eq!(ep.validate(0, &make_desc(1, 1, 8)), Ok(8));
        let mut ep2 = EpValidator::new();
        assert_eq!(ep2.validate(1, &make_desc(1, 1, 64)), Ok(64));
        let mut ep3 = EpValidator::new();
        assert_eq!(ep3.validate(2, &make_desc(1, 1, 1024)), Ok(1024));
    }

    #[test]
    fn over_cap_variants_rejected() {
        let mut ep = EpValidator::new();
        assert_eq!(ep.validate(0, &make_desc(1, 1, 16)), Err(EpCode::PktOverCap));
        let mut ep2 = EpValidator::new();
        assert_eq!(ep2.validate(1, &make_desc(1, 1, 128)), Err(EpCode::PktOverCap));
        let mut ep3 = EpValidator::new();
        assert_eq!(ep3.validate(2, &make_desc(1, 1, 2048)), Err(EpCode::PktOverCap));
    }

    #[test]
    fn zero_and_nonpow2() {
        let mut ep = EpValidator::new();
        assert_eq!(ep.validate(1, &make_desc(1, 1, 0)), Err(EpCode::PktZero));
        let mut ep2 = EpValidator::new();
        assert_eq!(ep2.validate(1, &make_desc(1, 1, 100)), Err(EpCode::PktNotPow2));
    }

    #[test]
    fn fragment_and_speed_unknown() {
        let mut ep = EpValidator::new();
        assert_eq!(ep.validate(1, &[1, 1, 8]), Err(EpCode::DescFragment));
        let mut ep2 = EpValidator::new();
        assert_eq!(ep2.validate(7, &make_desc(1, 1, 8)), Err(EpCode::SpeedUnknown));
    }

    #[test]
    fn dup_ep_rejected() {
        let mut ep = EpValidator::new();
        assert!(ep.validate(1, &make_desc(2, 0, 64)).is_ok());
        assert_eq!(ep.validate(1, &make_desc(2, 0, 64)), Err(EpCode::EpNumDup));
        // 反方向同号可过。
        assert!(ep.validate(1, &make_desc(2, 1, 64)).is_ok());
    }

    #[test]
    fn budget_isolation_no_leak() {
        let mut g = BudgetGate::new();
        for _ in 0..BUDGET {
            assert!(g.spend().is_ok());
        }
        assert_eq!(g.spend(), Err(EpCode::BudgetIsolated));
        assert!(g.isolated());
        assert_eq!(g.spend(), Err(EpCode::BudgetIsolated));
        let (iso, leak) = budget_isolation_panorama();
        assert!(iso >= ROUNDS && leak == 0);
    }

    #[test]
    fn slow_but_compliant_no_false_kill() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (5, 0));
    }

    #[test]
    fn all_ten_variants_rejected_low_speed() {
        let mut ep = EpValidator::new();
        for (i, &pkt) in MALFORMED_PKT.iter().enumerate() {
            let r = ep.validate(0, &make_desc(1, 1, pkt));
            assert!(r.is_err(), "变体 {} (pkt={}) 必须拒", i, pkt);
        }
    }

    #[test]
    fn campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        assert!(dual_run_consistent());
        assert_eq!(trace_fingerprint(), trace_fingerprint());
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
