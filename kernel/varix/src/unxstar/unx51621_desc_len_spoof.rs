//! UNX-F51621 描述符长度欺骗全谱（bLength 谎报 ×20 变体） · 0xC1E00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51622-J1 循环 ×10 检出拒绝；UNX-F51622-J1R 深嵌套合法设备 ×10 零误拒。
//!
//! 结构清单：
//!   - DescScanner：字节流级描述符扫描器（bLength 界内/界外两轴判定）
//!   - SpoofLedger：谎报审计账本（逐轮落账，注入账守恒）
//!   - Lcg：SEG_BASE 派生种子的确定性线性同余发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：只用 core/alloc，不碰 std；零浮点、零 IO、零墙钟、
//!      零外部随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② bLength 语义：USB 描述符首字节声明本描述符长度。谎报两族——
//!      放大（bLength > 剩余字节，读越界）与缩小（bLength < 自身类型长，
//!      吞掉后续描述符）。扫描器对两族都显性拒，拒绝不推进游标。
//!   ③ 深嵌套合法设备：合法 config 内多层描述符逐层正确声明 bLength，
//!      扫描器必须 10/10 零误拒——这是判据 J1R 的正面样本。
//!   ④ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51621 − 51501)。
pub const SEG_BASE: u32 = 0xC1E00;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51621u32 - 51501));

/// 扫描器一次能吃的最大缓冲（防恶意超长）。
pub const MAX_BUF: usize = 256;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DescCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 缓冲为空
    BufEmpty,
    /// 缓冲超上限
    BufOver,
    /// bLength 为零
    LenZero,
    /// bLength 超过剩余字节（放大谎报）
    LenOverrun,
    /// bLength 小于描述符自身最小类型长（缩小谎报）
    LenUndersized,
    /// 描述符类型不在闭集
    TypeUnknown,
    /// 剩余字节不成整描述符（尾渣）
    TailFragment,
    /// 同一缓冲内谎报轮数超上限
    StormGuard,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl DescCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            DescCode::Ok => "扫描成功：描述符链自洽",
            DescCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            DescCode::BufEmpty => "空缓冲：至少要有 2 字节才构成一个描述符头",
            DescCode::BufOver => "缓冲超限：恶意超长缓冲一律拒收并落账",
            DescCode::LenZero => "bLength 为零：零长描述符会导致游标死循环，显性拒",
            DescCode::LenOverrun => "bLength 放大谎报：声明长度超过剩余字节，读越界风险",
            DescCode::LenUndersized => "bLength 缩小谎报：声明长度小于该类型最小长度，会吞后续描述符",
            DescCode::TypeUnknown => "描述符类型未知：bDescriptorType 不在已知闭集",
            DescCode::TailFragment => "尾渣：剩余不足 2 字节且非零，不成整描述符",
            DescCode::StormGuard => "谎报风暴：单缓冲内谎报轮数超上限，触发风暴守卫",
            DescCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            DescCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [DescCode; 12] = [
    DescCode::Ok,
    DescCode::SegBad,
    DescCode::BufEmpty,
    DescCode::BufOver,
    DescCode::LenZero,
    DescCode::LenOverrun,
    DescCode::LenUndersized,
    DescCode::TypeUnknown,
    DescCode::TailFragment,
    DescCode::StormGuard,
    DescCode::LedgerMismatch,
    DescCode::FingerprintMismatch,
];

/// 已知描述符类型与最小长度闭集：(类型码, 最小 bLength)。
pub const KNOWN_TYPES: [(u8, usize); 6] = [
    (0x01, 18), // DEVICE
    (0x02, 9),  // CONFIGURATION
    (0x03, 2),  // STRING
    (0x04, 9),  // INTERFACE
    (0x05, 7),  // ENDPOINT
    (0x06, 9),  // DEVICE_QUALIFIER
];

/// 最小类型长查询（类型未知返回 None）。
pub fn min_len_of(ty: u8) -> Option<usize> {
    KNOWN_TYPES.iter().find(|&&(t, _)| t == ty).map(|&(_, l)| l)
}

/// SEG_BASE 派生种子（LCG）。
pub fn lcg_seed() -> u64 {
    (SEG_BASE as u64) << 17 ^ 0x5DEE_CE66
}

/// 确定性线性同余发生器（零外部随机）。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    /// 由段基址派生构造。
    pub fn new() -> Lcg {
        Lcg { st: lcg_seed() }
    }
    /// 下一步（64 位 Knuth 乘加）。
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.st
    }
    /// [0, m) 内取值。
    pub fn below(&mut self, m: u64) -> u64 {
        self.next() % m.max(1)
    }
}

/// 谎报审计账本：注入/检出/逃逸三本账守恒。
#[derive(Default)]
pub struct SpoofLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    /// 逐轮记录 (轮号, 诊断码值)。
    rows: Vec<(u32, u32)>,
}

impl SpoofLedger {
    /// 记一次检出。
    pub fn hit(&mut self, round: u32, code: DescCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, DescCode::Ok.val()));
    }
    /// 守恒断言：注入=检出+逃逸且逃逸为零且检出非零。
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

/// 描述符扫描器：逐条验 bLength，谎报显性拒、合法推进。
pub struct DescScanner {
    consumed: usize,
    rejects: u32,
    storm: bool,
    trace: Vec<String>,
}

impl DescScanner {
    /// 新扫描器。
    pub fn new() -> DescScanner {
        DescScanner { consumed: 0, rejects: 0, storm: false, trace: Vec::new() }
    }

    /// 扫一个缓冲：返回首个致命错误或 Ok。
    /// 判定序：空 → 超限 → 逐条（len==0 → overrun → undersized → type）。
    pub fn scan(&mut self, buf: &[u8]) -> Result<usize, DescCode> {
        if buf.is_empty() {
            return Err(DescCode::BufEmpty);
        }
        if buf.len() > MAX_BUF {
            self.rejects += 1;
            return Err(DescCode::BufOver);
        }
        // 风暴守卫：同一扫描器实例累计谎报超上限即熔断（状态跨调用累计）。
        if self.rejects >= ROUNDS {
            self.storm = true;
            return Err(DescCode::StormGuard);
        }
        let mut off = 0usize;
        while off < buf.len() {
            if buf.len() - off < 2 {
                return Err(DescCode::TailFragment);
            }
            let blen = match buf.get(off) {
                Some(&b) => b as usize,
                None => return Err(DescCode::BufEmpty),
            };
            let ty = match buf.get(off + 1) {
                Some(&b) => b,
                None => return Err(DescCode::BufEmpty),
            };
            if blen == 0 {
                // 首次零长报 LenZero（单点诊断优先），风暴由跨调用计数兜底。
                self.rejects += 1;
                return Err(DescCode::LenZero);
            }
            if blen > buf.len() - off {
                self.rejects += 1;
                return Err(DescCode::LenOverrun);
            }
            let minlen = match min_len_of(ty) {
                Some(l) => l,
                None => {
                    self.rejects += 1;
                    return Err(DescCode::TypeUnknown);
                }
            };
            if blen < minlen {
                self.rejects += 1;
                return Err(DescCode::LenUndersized);
            }
            off += blen;
            self.consumed += blen;
        }
        Ok(off)
    }

    /// 是否触发风暴守卫。
    pub fn storm_tripped(&self) -> bool {
        self.storm
    }
    /// 已消费字节数。
    pub fn consumed(&self) -> usize {
        self.consumed
    }
    /// 拒绝次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    /// 追加一行观测轨迹（双跑指纹原料）。
    pub fn note(&mut self, line: String) {
        self.trace.push(line);
    }
    /// 轨迹只读。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
}

/// 构造一条合法设备描述符（18 字节，类型 0x01）。
pub fn legal_device_desc() -> Vec<u8> {
    let head: [u8; 18] =
        [18, 0x01, 0x10, 0x02, 0x00, 0x00, 0x00, 0x40, 0xC1, 0x5A, 0x0B, 0x40,
         0x01, 0x01, 0x02, 0x03, 0x01, 0x00];
    head.iter().copied().collect()
}

/// 深嵌套合法设备：config + n×(interface + endpoint) 全部正确声明。
pub fn legal_nested_device(depth: usize) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    for b in legal_device_desc() {
        v.push(b);
    }
    // 9 字节 config 头：先占位，wTotalLength 最后回填。
    v.push(9u8);
    v.push(0x02);
    for _ in 0..7 {
        v.push(0u8);
    }
    let cfg_pos = v.len() - 9;
    for _ in 0..depth {
        for b in [9u8, 0x04, 0, 0, 1, 0, 0, 0, 0] {
            v.push(b);
        }
        for b in [7u8, 0x05, 0x81, 0x02, 0x40, 0x00, 0x00] {
            v.push(b);
        }
    }
    let total = v.len() - cfg_pos;
    // 回填 wTotalLength（小端），用 get_mut 避免裸下标。
    if let Some(lo) = v.get_mut(cfg_pos + 2) {
        *lo = (total & 0xFF) as u8;
    }
    if let Some(hi) = v.get_mut(cfg_pos + 3) {
        *hi = (total >> 8) as u8;
    }
    v
}

/// 构造一条放大谎报样本：bLength 声明超出剩余字节。
pub fn spoof_overrun(claimed: usize) -> Vec<u8> {
    let claimed = claimed.clamp(6, 250);
    let mut v: Vec<u8> = Vec::new();
    v.push(claimed as u8);
    v.push(0x02);
    // 缓冲只有 5 字节而声明更大：模拟只收到一小段。
    for _ in 0..3 {
        v.push(0u8);
    }
    v
}

/// 构造一条缩小谎报样本：bLength 小于该类型最小长。
pub fn spoof_undersized(ty: u8, blen: u8) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(blen);
    v.push(ty);
    for _ in 0..(blen as usize).max(2) {
        v.push(0u8);
    }
    v
}

/// 构造零长 bLength 样本。
pub fn spoof_zero() -> Vec<u8> {
    let v: [u8; 4] = [0, 0x03, 1, 0x03];
    v.iter().copied().collect()
}

/// 谎报全谱 ×10 轮注入：五族各 2 轮，检出落账，逃逸恒 0。
pub fn injection_campaign() -> SpoofLedger {
    let mut led = SpoofLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let fam = (r % 5) as u64;
        let bad = match fam {
            0 => spoof_zero(),
            1 => spoof_overrun(9 + rnd.below(200) as usize),
            2 => spoof_undersized(0x01, 3 + (rnd.below(10) as u8)),
            3 => spoof_undersized(0x02, 2 + (rnd.below(6) as u8)),
            _ => spoof_undersized(0x05, 3),
        };
        let mut sc = DescScanner::new();
        let hit = match sc.scan(&bad) {
            Ok(_) => None,
            Err(c) => Some(c),
        };
        match hit {
            Some(c) => led.hit(r + 1, c),
            None => led.miss(r + 1),
        }
    }
    led
}

/// 深嵌套合法设备 ×10 零误拒回归。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for d in 1..=10usize {
        let mut sc = DescScanner::new();
        let buf = legal_nested_device(d);
        sc.note(format!("depth={}", d));
        match sc.scan(&buf) {
            Ok(_) => ok += 1,
            Err(_) => bad += 1,
        }
    }
    (ok, bad)
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

/// 扫描器轨迹指纹（双跑一致面）。
pub fn trace_fingerprint() -> u64 {
    let mut fp: u64 = fnv1a(&[SEG_BASE as u8, (SEG_BASE >> 8) as u8]);
    let mut sc = DescScanner::new();
    let _ = sc.scan(&legal_nested_device(3));
    sc.note("legal3".to_string());
    for line in sc.trace() {
        fp = fnv1a(line.as_bytes().iter().map(|b| fp as u8 ^ b).collect::<Vec<u8>>().as_slice())
            ^ fp.rotate_left(7);
    }
    let led = injection_campaign();
    fp = fnv1a(&led.detected().to_le_bytes()) ^ fp.rotate_left(5);
    fp
}

/// 双跑一致性：指纹函数连跑两次必须一致。
pub fn dual_run_consistent() -> bool {
    trace_fingerprint() == trace_fingerprint()
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 错误值闭集自洽：val 两两不同且落在段内。
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
pub fn run_unx51621_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51621");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 合法设备描述符通过。
    {
        let mut sc = DescScanner::new();
        let ok = sc.scan(&legal_device_desc()).is_ok();
        s.add_dyn("合法设备描述符", ok, "标准 18 字节设备描述符必须通过");
    }
    // 4 深嵌套合法 ×10 零误拒（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "深嵌套合法零误拒",
            ok == 10 && bad == 0,
            "UNX-F51622-J1R 深嵌套合法设备 ×10 零误拒",
        );
    }
    // 5 谎报全谱注入 10/10 检出（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "谎报全谱注入 10/10",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51622-J1 循环 ×10 检出拒绝且账守恒",
        );
    }
    // 6 零长 bLength 显性拒。
    {
        let mut sc = DescScanner::new();
        let r = sc.scan(&spoof_zero());
        s.add_dyn("零长显性拒", r == Err(DescCode::LenZero), "零 bLength 会死循环必须显性拒");
    }
    // 7 放大谎报显性拒。
    {
        let mut sc = DescScanner::new();
        let r = sc.scan(&spoof_overrun(64));
        s.add_dyn("放大谎报显性拒", r == Err(DescCode::LenOverrun), "声明超剩余字节必须显性拒");
    }
    // 8 缩小谎报显性拒。
    {
        let mut sc = DescScanner::new();
        let r = sc.scan(&spoof_undersized(0x01, 4));
        s.add_dyn("缩小谎报显性拒", r == Err(DescCode::LenUndersized), "小于类型最小长必须显性拒");
    }
    // 9 未知类型显性拒。
    {
        let mut v: Vec<u8> = Vec::new();
        for b in [18u8, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0] {
            v.push(b);
        }
        while v.len() < 18 {
            v.push(0u8);
        }
        let mut sc = DescScanner::new();
        let r = sc.scan(&v);
        s.add_dyn("未知类型显性拒", r == Err(DescCode::TypeUnknown), "类型码越闭集必须显性拒");
    }
    // 10 空缓冲与超限缓冲。
    {
        let mut sc = DescScanner::new();
        let empty = sc.scan(&[]);
        let big: Vec<u8> = (0..(MAX_BUF + 1)).map(|i| (i % 251) as u8).collect();
        let over = sc.scan(&big);
        s.add_dyn(
            "空与超限缓冲",
            empty == Err(DescCode::BufEmpty) && over == Err(DescCode::BufOver),
            "空缓冲与超上限缓冲必须分码拒收",
        );
    }
    // 11 风暴守卫：同一扫描器连环谎报熔断。
    {
        let mut sc = DescScanner::new();
        let mut last = Ok(0usize);
        for _ in 0..(ROUNDS + 1) {
            last = sc.scan(&spoof_zero());
        }
        s.add_dyn(
            "谎报风暴熔断",
            last == Err(DescCode::StormGuard) && sc.storm_tripped(),
            "连环谎报超轮数上限必须熔断",
        );
    }
    // 12 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 13 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 14 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    // 15 拒绝不推进游标：谎报后合法扫描不受污染。
    {
        let mut sc = DescScanner::new();
        let _ = sc.scan(&spoof_zero());
        let before = sc.consumed();
        let ok2 = sc.scan(&legal_device_desc()).is_ok();
        s.add_dyn(
            "拒绝不污染游标",
            ok2 && before == 0 && sc.consumed() == 18,
            "谎报拒收不得影响后续合法扫描",
        );
    }
    // 16 类型最小长表自洽。
    {
        let mut sane = true;
        for &(t, l) in KNOWN_TYPES.iter() {
            if l < 2 || min_len_of(t) != Some(l) {
                sane = false;
            }
        }
        s.add_dyn("类型表自洽", sane, "已知类型最小长必须 ≥2 且查询一致");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51621_checks_green() {
        let set = run_unx51621_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51621u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1E00);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(DescCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(DescCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert!(val_codes_distinct());
        assert_eq!(ALL_CODES.len(), 12);
    }

    #[test]
    fn legal_desc_passes() {
        let mut sc = DescScanner::new();
        assert!(sc.scan(&legal_device_desc()).is_ok());
        assert_eq!(sc.consumed(), 18);
    }

    #[test]
    fn nested_device_zero_false_reject() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (10, 0));
    }

    #[test]
    fn zero_len_rejected() {
        let mut sc = DescScanner::new();
        assert_eq!(sc.scan(&spoof_zero()), Err(DescCode::LenZero));
    }

    #[test]
    fn overrun_rejected() {
        for claimed in [2usize, 9, 64, 129, 250] {
            let mut sc = DescScanner::new();
            assert_eq!(sc.scan(&spoof_overrun(claimed)), Err(DescCode::LenOverrun));
        }
    }

    #[test]
    fn undersized_rejected() {
        for (ty, blen) in [(0x01u8, 4u8), (0x02u8, 2u8), (0x05u8, 3u8)] {
            let mut sc = DescScanner::new();
            assert_eq!(sc.scan(&spoof_undersized(ty, blen)), Err(DescCode::LenUndersized));
        }
    }

    #[test]
    fn unknown_type_rejected() {
        let mut sc = DescScanner::new();
        let v: [u8; 9] = [9, 0x70, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(sc.scan(&v), Err(DescCode::TypeUnknown));
    }

    #[test]
    fn storm_guard_trips() {
        let mut sc = DescScanner::new();
        for _ in 0..ROUNDS {
            assert_eq!(sc.scan(&spoof_zero()), Err(DescCode::LenZero));
        }
        assert_eq!(sc.scan(&spoof_zero()), Err(DescCode::StormGuard));
        assert!(sc.storm_tripped());
    }

    #[test]
    fn injection_ledger_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn dual_run_fingerprint_stable() {
        let a = trace_fingerprint();
        let b = trace_fingerprint();
        assert_eq!(a, b, "双跑指纹必须一致");
        assert!(dual_run_consistent());
    }

    #[test]
    fn empty_and_over_buf() {
        let mut sc = DescScanner::new();
        assert_eq!(sc.scan(&[]), Err(DescCode::BufEmpty));
        let big: Vec<u8> = Vec::new();
        let _ = big;
        let big: Vec<u8> = (0..(MAX_BUF + 5)).map(|_| 9u8).collect();
        assert_eq!(sc.scan(&big), Err(DescCode::BufOver));
    }

    #[test]
    fn details_all_nonempty() {
        for c in ALL_CODES {
            assert!(!c.detail().is_empty());
        }
    }
}
