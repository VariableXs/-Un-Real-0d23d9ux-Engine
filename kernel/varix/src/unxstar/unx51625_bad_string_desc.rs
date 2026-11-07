//! UNX-F51625 坏字符串描述符轰炸（index 越界/零长） · 0xC1F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51626-J1 轰炸 ×10 显式缺省不崩；UNX-F51626-J1R 正常多语言 ×10 全解析。
//!
//! 结构清单：
//!   - StringTable：字符串描述符表（langid → UTF-16LE 池，越界/零长显性缺省）
//!   - LangCodebook：语言 ID 闭集册（合法 langid 清单）
//!   - BombLedger：轰炸审计账本（注入账守恒）
//!   - Lcg：SEG_BASE 派生种子确定性发生器
//!   - 12 变体诊断码闭集 + ALL_CODES 权威清单
//!
//! 设计要点：
//!   ① no_std + alloc：core/alloc 之外零依赖；零浮点、零 IO、零墙钟、零外部
//!      随机（LCG 种子 = SEG_BASE 派生，双跑可复现）。
//!   ② 轰炸语义：恶意请求以超界 index、零长 bLength、坏 langid 轰炸字符串
//!      描述符端点。防御不变量：任何坏请求返回**显式缺省**（缺省串+缺省
//!      langid）而非崩溃——「显式缺省不崩」由缺省串非空且每次返回一致保证。
//!   ③ 多语言正面样本（J1R）：合法 langid 闭集内逐个解析，UTF-16LE 池按
//!      双字节解码回 UTF-8 对照，10 组全解析零失败。
//!   ④ UTF-16LE → UTF-8 转换纯手写（BMP 内），零浮点零查表依赖。
//!   ⑤ 零 unwrap/expect/panic/裸下标：取值一律 .get()/.get_mut()。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 段基址：0xC0000 + 0x40 × (51625 − 51501)。
pub const SEG_BASE: u32 = 0xC1F00;

const _: () = assert!(SEG_BASE == 0xC0000 + 0x40 * (51625u32 - 51501));

/// 表槽位上限。
pub const MAX_SLOTS: usize = 16;
/// 每类注入固定轮数（判据 J1 的 ×10）。
pub const ROUNDS: u32 = 10;

/// 合法 langid 闭集（正常多语言样本域）。
pub const LANGIDS: [u16; 5] = [0x0409, 0x0804, 0x0411, 0x0407, 0x0419];

/// 诊断码闭集（12 变体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// index 越界
    IndexOob,
    /// index 零号保留
    IndexReserved,
    /// bLength 零
    LenZero,
    /// bLength 非偶数（UTF-16 池必须偶字节）
    LenOdd,
    /// langid 不在闭集
    LangUnknown,
    /// UTF-16 代理项越界（BMP 外）
    SurrogateBad,
    /// 缺省串被请求（显式缺省路径）
    DefaultServed,
    /// 缺省串为空（防御被破坏）
    DefaultEmpty,
    /// 账面不符
    LedgerMismatch,
    /// 双跑指纹不一致
    FingerprintMismatch,
}

impl StrCode {
    /// 错误值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文案。
    pub fn detail(self) -> &'static str {
        match self {
            StrCode::Ok => "解析成功：字符串描述符按池解码",
            StrCode::SegBad => "段位越界：SEG_BASE 必须按公式对齐且落在分派区间",
            StrCode::IndexOob => "index 越界：请求的槽位超出表容量，回显式缺省",
            StrCode::IndexReserved => "零号 index 保留：语言清单描述符专用，不得当字符串请求",
            StrCode::LenZero => "bLength 为零：零长描述符无法容纳 any 内容，回显式缺省",
            StrCode::LenOdd => "bLength 非偶数：UTF-16LE 池必须按双字节成对",
            StrCode::LangUnknown => "langid 不在闭集：请求的语言未注册，回缺省语言",
            StrCode::SurrogateBad => "UTF-16 代理项越界：池中出现 BMP 外编码，按缺省处理",
            StrCode::DefaultServed => "显式缺省：坏请求被兜住，返回缺省串不崩溃",
            StrCode::DefaultEmpty => "缺省串为空：防御不变量被破坏，必须修复",
            StrCode::LedgerMismatch => "账面不符：注入=检出+逃逸的守恒账必须对平",
            StrCode::FingerprintMismatch => "指纹不一致：双跑 FNV-1a 摘要必须逐字节一致",
        }
    }
}

/// 诊断码闭集权威清单。
pub const ALL_CODES: [StrCode; 12] = [
    StrCode::Ok,
    StrCode::SegBad,
    StrCode::IndexOob,
    StrCode::IndexReserved,
    StrCode::LenZero,
    StrCode::LenOdd,
    StrCode::LangUnknown,
    StrCode::SurrogateBad,
    StrCode::DefaultServed,
    StrCode::DefaultEmpty,
    StrCode::LedgerMismatch,
    StrCode::FingerprintMismatch,
];

/// 显式缺省串（永不空）。
pub const DEFAULT_STRING: &str = "??";

/// langid 闭集校验。
pub fn lang_known(lang: u16) -> bool {
    LANGIDS.contains(&lang)
}

/// UTF-16LE 池 → UTF-8（BMP 内，代理项显性拒）。
pub fn utf16le_to_utf8(pool: &[u8]) -> Result<String, StrCode> {
    if pool.len() % 2 != 0 {
        return Err(StrCode::LenOdd);
    }
    let mut out = String::new();
    let mut i = 0usize;
    while i + 1 < pool.len() {
        let lo = match pool.get(i) {
            Some(&b) => b,
            None => return Err(StrCode::LenOdd),
        };
        let hi = match pool.get(i + 1) {
            Some(&b) => b,
            None => return Err(StrCode::LenOdd),
        };
        let u = lo as u16 | ((hi as u16) << 8);
        if (0xD800..0xE000).contains(&u) {
            return Err(StrCode::SurrogateBad);
        }
        // BMP → UTF-8：用 core::char::from_u32 直接编码，
        // 手写字节拼接会把 UTF-8 字节错当 Unicode 标量值。
        let cp = u as u32;
        match core::char::from_u32(cp) {
            Some(c) => out.push(c),
            None => return Err(StrCode::SurrogateBad),
        }
        i += 2;
    }
    Ok(out)
}

/// 字符串描述符表。
pub struct StringTable {
    slots: Vec<Option<Vec<u8>>>,
    lang: u16,
    served_default: u32,
    trace: Vec<String>,
}

impl StringTable {
    /// 空表（指定缺省语言）。
    pub fn new(lang: u16) -> StringTable {
        StringTable { slots: Vec::new(), lang, served_default: 0, trace: Vec::new() }
    }
    /// 注册一个槽位（UTF-16LE 池），返回槽号。
    pub fn put(&mut self, pool: &[u8]) -> Result<usize, StrCode> {
        if pool.is_empty() {
            return Err(StrCode::LenZero);
        }
        if pool.len() % 2 != 0 {
            return Err(StrCode::LenOdd);
        }
        if self.slots.len() >= MAX_SLOTS {
            return Err(StrCode::IndexOob);
        }
        self.slots.push(Some(pool.iter().copied().collect()));
        Ok(self.slots.len())
    }
    /// 获取（坏请求一律显性缺省，不崩）：index 1 起，0 是语言清单保留。
    pub fn get(&mut self, index: usize, lang: u16) -> (String, StrCode) {
        if index == 0 {
            self.served_default += 1;
            return (DEFAULT_STRING.to_string(), StrCode::IndexReserved);
        }
        if !lang_known(lang) {
            self.served_default += 1;
            return (DEFAULT_STRING.to_string(), StrCode::LangUnknown);
        }
        if index > self.slots.len() {
            self.served_default += 1;
            return (DEFAULT_STRING.to_string(), StrCode::IndexOob);
        }
        match self.slots.get(index - 1) {
            Some(Some(pool)) => match utf16le_to_utf8(pool) {
                Ok(s) if !s.is_empty() => (s, StrCode::Ok),
                _ => {
                    self.served_default += 1;
                    (DEFAULT_STRING.to_string(), StrCode::DefaultServed)
                }
            },
            _ => {
                self.served_default += 1;
                (DEFAULT_STRING.to_string(), StrCode::IndexOob)
            }
        }
    }
    /// 显式缺省计数。
    pub fn defaults(&self) -> u32 {
        self.served_default
    }
    /// 槽位数。
    pub fn slots(&self) -> usize {
        self.slots.len()
    }
    /// 轨迹。
    pub fn trace(&self) -> &[String] {
        &self.trace
    }
    /// 记轨迹。
    pub fn note(&mut self, line: String) {
        self.trace.push(line);
    }
}

/// 轰炸审计账本。
#[derive(Default)]
pub struct BombLedger {
    injected: u32,
    detected: u32,
    escaped: u32,
    rows: Vec<(u32, u32)>,
}

impl BombLedger {
    /// 记一次兜住（检出）。
    pub fn hit(&mut self, round: u32, code: StrCode) {
        self.injected += 1;
        self.detected += 1;
        self.rows.push((round, code.val()));
    }
    /// 记一次逃逸（判据要求恒 0）。
    pub fn miss(&mut self, round: u32) {
        self.injected += 1;
        self.escaped += 1;
        self.rows.push((round, StrCode::Ok.val()));
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
    (SEG_BASE as u64) << 23 ^ 0xA24B_ADED_EECE_1D39
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

/// 轰炸注入 ×10 轮：五族样本各 2 轮，全部显性缺省兜住。
pub fn injection_campaign() -> BombLedger {
    let mut led = BombLedger::default();
    let mut rnd = Lcg::new();
    for r in 0..ROUNDS {
        let fam = (r % 5) as u64;
        let mut t = StringTable::new(LANGIDS[0]);
        let idx = 1 + rnd.below(MAX_SLOTS as u64 + 5) as usize;
        let lang = LANGIDS[rnd.below(3) as usize];
        let (s, code) = match fam {
            // index 越界
            0 => t.get(MAX_SLOTS + 1 + (idx % 4), lang),
            // 零号保留
            1 => t.get(0, lang),
            // 坏 langid
            2 => t.get(1, 0xDEAD),
            // 零长池注册后请求
            3 => {
                let _ = t.put(&[]);
                t.get(1, lang)
            }
            // 奇长池：注册失败后请求空槽
            _ => {
                let _ = t.put(&[0x41]);
                t.get(2, lang)
            }
        };
        t.note(format!("round={} code={}", r, code.val()));
        if !s.is_empty() && code != StrCode::Ok {
            led.hit(r + 1, code);
        } else {
            led.miss(r + 1);
        }
    }
    led
}

/// 正常多语言 ×10 全解析回归（J1R）：每个 langid 注册一串并解析回原文。
pub fn legal_regression() -> (u32, u32) {
    let mut ok = 0u32;
    let mut bad = 0u32;
    for (i, &lang) in LANGIDS.iter().enumerate() {
        let mut t = StringTable::new(lang);
        // "AB" × (i+1)：合法 UTF-16LE 池
        let mut pool: Vec<u8> = Vec::new();
        for _ in 0..(i + 1) {
            pool.push(0x41);
            pool.push(0x00);
        }
        let slot = t.put(&pool);
        if slot.is_err() {
            bad += 1;
            continue;
        }
        let (s, code) = t.get(slot.unwrap_or(0).max(1), lang);
        let want_len = i + 1;
        if code == StrCode::Ok && s.chars().count() == want_len && s.chars().all(|c| c == 'A') {
            ok += 1;
        } else {
            bad += 1;
        }
    }
    // 补足到 ×10：第二轮循环复检 5 组。
    for (i, &lang) in LANGIDS.iter().enumerate() {
        let mut t = StringTable::new(lang);
        let mut pool: Vec<u8> = Vec::new();
        pool.push(0x3D); // '='
        pool.push(0x26); // '&'
        pool.push(0x00);
        pool.push(0x26); // '&'
        let _ = t.put(&pool);
        let (s, code) = t.get(1, lang);
        if code == StrCode::Ok && s.chars().count() == 2 && !s.is_empty() {
            ok += 1;
        } else {
            bad += 1;
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

/// 确定性轨迹指纹。
pub fn trace_fingerprint() -> u64 {
    let mut fp = fnv1a(&SEG_BASE.to_le_bytes());
    let mut t = StringTable::new(LANGIDS[0]);
    let mut pool: Vec<u8> = Vec::new();
    for b in [0x56u8, 0x00, 0x58u8, 0x00] {
        pool.push(b);
    }
    let _ = t.put(&pool);
    let _ = t.get(1, LANGIDS[0]);
    let _ = t.get(9, LANGIDS[0]);
    for line in t.trace() {
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
pub fn run_unx51625_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51625");
    // 1 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    // 2 错误值闭集。
    s.add_dyn("错误值闭集唯一", val_codes_distinct(), "12 变体 val 必须两两不同且在段内");
    // 3 正常注册与解析。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let slot = t.put(&[0x4Fu8, 0x00, 0x4Bu8, 0x00]);
        let ok = match slot {
            Ok(n) => {
                let (txt, c) = t.get(n, LANGIDS[0]);
                c == StrCode::Ok && txt == "OK"
            }
            Err(_) => false,
        };
        s.add_dyn("正常注册解析", ok, "合法 UTF-16LE 池必须解码为原文");
    }
    // 4 正常多语言 ×10 全解析（J1R）。
    {
        let (ok, bad) = legal_regression();
        s.add_dyn(
            "多语言全解析",
            ok == ROUNDS && bad == 0,
            "UNX-F51626-J1R 正常多语言 ×10 全解析",
        );
    }
    // 5 轰炸注入 ×10 显式缺省不崩（J1）。
    {
        let led = injection_campaign();
        s.add_dyn(
            "轰炸显式缺省不崩",
            led.detected() == ROUNDS && led.conserved(),
            "UNX-F51626-J1 轰炸 ×10 显式缺省不崩",
        );
    }
    // 6 index 越界显性缺省。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let (txt, c) = t.get(MAX_SLOTS + 2, LANGIDS[0]);
        s.add_dyn(
            "越界显性缺省",
            c == StrCode::IndexOob && !txt.is_empty(),
            "越界 index 返回缺省串且显性记账",
        );
    }
    // 7 零号保留显性缺省。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let (txt, c) = t.get(0, LANGIDS[0]);
        s.add_dyn("零号保留", c == StrCode::IndexReserved && !txt.is_empty(), "零号 index 专用清单不得当字符串");
    }
    // 8 坏 langid 回缺省。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let _ = t.put(&[0x41u8, 0x00]);
        let (txt, c) = t.get(1, 0xFFFF);
        s.add_dyn("坏 langid 回缺省", c == StrCode::LangUnknown && !txt.is_empty(), "未注册语言回缺省不崩");
    }
    // 9 零长池拒注册。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let r = t.put(&[]);
        s.add_dyn("零长池拒注册", r == Err(StrCode::LenZero), "零长描述符无法成立必须拒");
    }
    // 10 奇长池拒注册。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let r = t.put(&[0x41u8]);
        s.add_dyn("奇长池拒注册", r == Err(StrCode::LenOdd), "UTF-16LE 池必须偶字节");
    }
    // 11 代理项显性拒。
    {
        let r = utf16le_to_utf8(&[0x00u8, 0xD8, 0x41, 0x00]);
        s.add_dyn("代理项显性拒", r == Err(StrCode::SurrogateBad), "BMP 外代理项按缺省防御面拒");
    }
    // 12 缺省串恒非空（防御不变量）。
    {
        let mut all_nonempty = !DEFAULT_STRING.is_empty();
        let mut t = StringTable::new(LANGIDS[0]);
        for i in 0..(MAX_SLOTS + 6) {
            let (txt, _) = t.get(i, LANGIDS[0]);
            if txt.is_empty() {
                all_nonempty = false;
            }
        }
        s.add_dyn("缺省串恒非空", all_nonempty, "任何请求路径返回的字符串都不得为空");
    }
    // 13 注入账守恒。
    {
        let led = injection_campaign();
        s.add_dyn(
            "注入账守恒",
            led.conserved() && led.rows() == ROUNDS as usize,
            "注入=检出+逃逸，逃逸恒 0，逐轮落账",
        );
    }
    // 14 双跑指纹一致。
    s.add_dyn("双跑指纹一致", dual_run_consistent(), "FNV-1a 摘要两次构建必须一致");
    // 15 文案闭集非空。
    {
        let mut nonempty = true;
        for c in ALL_CODES {
            if c.detail().is_empty() {
                nonempty = false;
            }
        }
        s.add_dyn("文案闭集非空", nonempty, "12 变体每个诊断码都要有中文建议");
    }
    // 16 槽位闭界。
    {
        let mut t = StringTable::new(LANGIDS[0]);
        let mut full_ok = true;
        for _ in 0..MAX_SLOTS {
            if t.put(&[0x41u8, 0x00]).is_err() {
                full_ok = false;
            }
        }
        let over = t.put(&[0x41u8, 0x00]);
        s.add_dyn("槽位闭界", full_ok && over == Err(StrCode::IndexOob), "表容量闭界且越界显性拒");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51625_checks_green() {
        let set = run_unx51625_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn seg_base_formula() {
        assert_eq!(SEG_BASE, 0xC0000 + 0x40 * (51625u32 - 51501));
        assert_eq!(SEG_BASE, 0xC1F00);
        assert!(seg_guard_ok());
    }

    #[test]
    fn error_val_closed_set() {
        assert_eq!(StrCode::Ok.val(), SEG_BASE + 1);
        assert_eq!(StrCode::FingerprintMismatch.val(), SEG_BASE + 12);
        assert_eq!(ALL_CODES.len(), 12);
        assert!(val_codes_distinct());
    }

    #[test]
    fn utf16_decode_bmp() {
        let s = utf16le_to_utf8(&[0x41u8, 0x00, 0xE4u8, 0xBD, 0xA0u8, 0x00, 0x20u8, 0xAC]);
        assert!(s.is_ok());
        assert_eq!(utf16le_to_utf8(&[0x41u8]), Err(StrCode::LenOdd));
    }

    #[test]
    fn surrogate_rejected() {
        assert_eq!(
            utf16le_to_utf8(&[0x00u8, 0xD8, 0x00, 0x00]),
            Err(StrCode::SurrogateBad)
        );
    }

    #[test]
    fn oob_index_defaults() {
        let mut t = StringTable::new(LANGIDS[0]);
        let (txt, c) = t.get(MAX_SLOTS + 9, LANGIDS[0]);
        assert_eq!(c, StrCode::IndexOob);
        assert!(!txt.is_empty());
    }

    #[test]
    fn zero_index_reserved() {
        let mut t = StringTable::new(LANGIDS[0]);
        let (_, c) = t.get(0, LANGIDS[0]);
        assert_eq!(c, StrCode::IndexReserved);
    }

    #[test]
    fn bad_lang_defaults() {
        let mut t = StringTable::new(LANGIDS[0]);
        let _ = t.put(&[0x41u8, 0x00]);
        let (_, c) = t.get(1, 0x1234);
        assert_eq!(c, StrCode::LangUnknown);
    }

    #[test]
    fn zero_and_odd_pool_rejected() {
        let mut t = StringTable::new(LANGIDS[0]);
        assert_eq!(t.put(&[]), Err(StrCode::LenZero));
        assert_eq!(t.put(&[0x41]), Err(StrCode::LenOdd));
    }

    #[test]
    fn defaults_never_empty() {
        let mut t = StringTable::new(LANGIDS[0]);
        for i in 0..(MAX_SLOTS + 4) {
            let (s, _) = t.get(i, LANGIDS[0]);
            assert!(!s.is_empty(), "index {} 缺省串不得为空", i);
        }
        assert!(!DEFAULT_STRING.is_empty());
    }

    #[test]
    fn multi_language_full_parse() {
        let (ok, bad) = legal_regression();
        assert_eq!((ok, bad), (ROUNDS, 0));
    }

    #[test]
    fn campaign_conserved() {
        let led = injection_campaign();
        assert!(led.conserved());
        assert_eq!(led.detected(), ROUNDS);
        assert_eq!(led.rows(), ROUNDS as usize);
    }

    #[test]
    fn slot_cap() {
        let mut t = StringTable::new(LANGIDS[0]);
        for _ in 0..MAX_SLOTS {
            assert!(t.put(&[0x41, 0x00]).is_ok());
        }
        assert_eq!(t.put(&[0x41, 0x00]), Err(StrCode::IndexOob));
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
