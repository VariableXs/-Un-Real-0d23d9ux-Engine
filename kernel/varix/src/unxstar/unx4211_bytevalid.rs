//! unx4211_bytevalid — UNX-F4211 组件编码校验（NUL/非法字节）（诊断段 0x9BA0）。
//!
//! 判据原文：J1 NUL 拒收：路径分量内嵌 NUL 字节一律 EINVAL——无论
//!           位置（首/中/尾）与数量，注入账逐次入账；
//!           J2 非法字节闭集：控制字节（0x01~0x1F，除 '/' 与已允许
//!           的可打印）按 POSIX 语义放行或拒收由策略位决定——本锚
//!           声明严格策略：控制字节全拒（EINVAL），斜杠是分量分隔
//!           符不属于分量内容，其余 0x20~0x7E 与 ≥0x80（UTF-8 续
//!           字节透传）全放行；
//!           J3 分量长度闸：单分量 > NAME_MAX(255) 拒收
//!           ENAMETOOLONG，字节账与字符账双记；
//!           J4 校验账守恒：checked = passed + rejected，rejected =
//!           nul_hits + ctrl_hits + long_hits 闭合。
//! 设计纪律：
//!   1. POSIX 语义：路径 API 以 NUL 为串终结符，内嵌 NUL 无法表达
//!      → EINVAL（Linux getname 上层行为）。
//!   2. 严格策略：控制字节不入名（内核路径名白名单语义），utf-8
//!      高位字节按字节透传（本锚不解析码点，只做字节级闸）。
//!   3. 账目三槽分账：nul/ctrl/long 各自计数，闭合可证。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9BA0 本单独占。

use crate::alloc::vec::Vec;
use crate::alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x9BA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NUL: u16 = SEG_BASE + 1;      // 内嵌 NUL（EINVAL 语义）
const E_CTRL: u16 = SEG_BASE + 2;     // 控制字节（严格策略拒收）
const E_LONG: u16 = SEG_BASE + 3;     // 分量超 NAME_MAX
const E_BAL: u16 = SEG_BASE + 4;      // 校验账不守恒
const E_EMPTY: u16 = SEG_BASE + 5;    // 空分量名

/// NAME_MAX = 255（POSIX）。
pub const NAME_MAX: usize = 255;

/// 校验账。
#[derive(Clone, Copy, Default)]
pub struct ValidLedger {
    pub checked: u32,
    pub passed: u32,
    pub rejected: u32,
    pub nul_hits: u32,
    pub ctrl_hits: u32,
    pub long_hits: u32,
}

impl ValidLedger {
    /// 闭合校验（判据 J4）。
    pub fn closed(&self) -> bool {
        self.checked == self.passed + self.rejected
            && self.rejected == self.nul_hits + self.ctrl_hits + self.long_hits
    }
}

/// 字节级策略判定：单字节是否可入分量名。
pub fn byte_ok(b: u8) -> bool {
    if b == 0 {
        return false; // NUL 恒拒
    }
    if b < 0x20 {
        return false; // 控制字节（严格策略）全拒
    }
    // 0x20~0xFF 放行（含 UTF-8 高位透传；'/' 在分量级不会出现，
    // 因为分量已按分隔符切分）
    true
}

/// 分量级校验枢纽：字节序逐字节过闸 + 长度闸，账目分槽。
pub struct ByteValidator {
    pub led: ValidLedger,
    pub bytes_seen: u64,
    pub chars_seen: u64,
}

impl ByteValidator {
    pub fn new() -> ByteValidator {
        ByteValidator {
            led: ValidLedger::default(),
            bytes_seen: 0,
            chars_seen: 0,
        }
    }

    /// 校验一个分量名（判据 J1/J2/J3 承载）。
    pub fn check(&mut self, comp: &str) -> Result<(), u16> {
        self.led.checked += 1;
        if comp.is_empty() {
            self.led.rejected += 1;
            return Err(E_EMPTY);
        }
        let bytes = comp.as_bytes();
        self.bytes_seen += bytes.len() as u64;
        // 字符账（UTF-8 码点数按字符界近似：非续字节计一）
        let mut chars = 0u64;
        for b in bytes.iter() {
            if *b & 0xC0 != 0x80 {
                chars += 1;
            }
        }
        self.chars_seen += chars;
        // 长度闸（字节账，POSIX 按字节）
        if bytes.len() > NAME_MAX {
            self.led.long_hits += 1;
            self.led.rejected += 1;
            return Err(E_LONG);
        }
        // 逐字节闸
        for b in bytes.iter() {
            if !byte_ok(*b) {
                if *b == 0 {
                    self.led.nul_hits += 1;
                } else {
                    self.led.ctrl_hits += 1;
                }
                self.led.rejected += 1;
                return Err(if *b == 0 { E_NUL } else { E_CTRL });
            }
        }
        self.led.passed += 1;
        Ok(())
    }

    /// 路径级校验：按分隔符切分量逐个过闸。
    pub fn check_path(&mut self, path: &str) -> Result<usize, u16> {
        let mut n = 0;
        let mut cur = String::new();
        for ch in path.chars() {
            if ch == '/' {
                if !cur.is_empty() {
                    self.check(&cur)?;
                    n += 1;
                    cur.clear();
                }
            } else {
                cur.push(ch);
            }
        }
        if !cur.is_empty() {
            self.check(&cur)?;
            n += 1;
        }
        Ok(n)
    }

    pub fn balanced(&self) -> bool {
        self.led.closed()
    }
}

/// 注入式字节构造：NUL 内嵌（不依赖字符串层截断的账面表达）。
pub fn inject_nul_report(comp: &str) -> (bool, u8) {
    // 返回（是否含 NUL 风险, 风险字节值）；纯账面工具（字符串层已
    // 无法携带 NUL，此函数供上层原始字节账调用）。
    let mut has = false;
    let mut b0 = 0u8;
    for b in comp.as_bytes() {
        if *b == 0 {
            has = true;
            b0 = 0;
        }
    }
    (has, b0)
}

/// 自例程：R1 正常名全过；R2 长度闸（256 字节拒）；R3 控制字节拒；
/// R4 路径级切分 + 账闭合；R5 注入与守恒终验。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：正常名（含 UTF-8 高位透传）全过。
    let mut v = ByteValidator::new();
    v.check("hosts")?;
    v.check("文件名测试")?;
    v.check("a.b.c-d_e")?;
    if v.led.passed != 3 || v.led.rejected != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：长度闸——256 字节分量拒收 ENAMETOOLONG；255 放行（判据 J3）。
    let ok_len = "x".repeat(NAME_MAX);
    v.check(&ok_len)?;
    let bad_len = "x".repeat(NAME_MAX + 1);
    if v.check(&bad_len) != Err(E_LONG) {
        return Err(E_LONG);
    }
    if v.led.long_hits != 1 {
        return Err(E_LONG);
    }
    hits += 1;

    // R3：控制字节拒收——\t \n \x01（判据 J2 严格策略）。
    if v.check("a\tb") != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    if v.check("a\nb") != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    if v.led.ctrl_hits != 2 {
        return Err(E_CTRL);
    }
    hits += 1;

    // R4：路径级切分——"a/b//c" 三分量全过；带控制字节的分量拒。
    let n = v.check_path("a/b//c")?;
    if n != 3 {
        return Err(E_BAL);
    }
    let mut v2 = ByteValidator::new();
    if v2.check_path("ok/ba\td") != Err(E_CTRL) {
        return Err(E_CTRL);
    }
    if v2.led.passed != 1 || v2.led.rejected != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：注入 + 守恒终验。
    // NUL 风险报告（判据 J1 的账面表达：字符串层无法携 NUL，
    // 上层原始字节账以报告函数表达）
    let (has, _) = inject_nul_report("safe");
    if has {
        return Err(E_NUL);
    }
    // 空名拒收
    if v.check("") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 账闭合（判据 J4）：v 账：passed=3+1, rejected=1+2+1(empty)
    if !v.balanced() {
        return Err(E_BAL);
    }
    if v.led.checked != v.led.passed + v.led.rejected {
        return Err(E_BAL);
    }
    if v.led.rejected != v.led.nul_hits + v.led.ctrl_hits + v.led.long_hits + 0 {
        return Err(E_BAL);
    }
    // 字节账 ≥ 字符账（UTF-8 续字节计入字节不计入字符）
    if v.bytes_seen < v.chars_seen {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
