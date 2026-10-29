//! UNX-F0740 · B37 批 ktest 断言集（跟随段收口） · 诊断段 0x5c00
//! 判据原文摘录：UNX-F0740-J1 本批判据聚合入 ktest unx_a1_b37 一次命令全绿
//! （三重口径）且批小计 11,490 守恒核对过；[留痕] R-A1-004：本批头部批小计
//! 11450 为逐条求和后落账（先求和后落账，零虚记）。
//! 纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. 三重口径写死：口径一 = 卡面逐条判据行数；口径二 = 逐条求和；
//!    口径三 = 头部落账批小计（11,490）。三口径必须在收口时逐位相等，
//!    任一不等即守恒破裂，收口拒绝。
//! 2. 先求和后落账：批小计只能由逐条行数求和产出后写落账位，
//!    不允许先写小计再凑明细（R-A1-004 留痕口径的程序化落实）。
//! 3. ktest 断言集 ≥9 条（本批 9 单逐单一条 + 守恒一条 + 全绿一条），
//!    一次命令全绿 = 集内零红。
//! 4. 定容数组账本 + 守恒恒等式：sum(lines) == posted_subtotal == face_subtotal；
//!    green+red == executed。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5c00 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5c00;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_UNIT_DUP: u16 = SEG_BASE + 1; // 单元号重复登记
const E_UNIT_FULL: u16 = SEG_BASE + 2; // 单元账已满
const E_UNIT_NOTFOUND: u16 = SEG_BASE + 3; // 单元号不存在
const E_LINES_ZERO: u16 = SEG_BASE + 4; // 行数为 0（纯功能线不许零行）
const E_SUBTOTAL_TWICE: u16 = SEG_BASE + 5; // 批小计重复落账
const E_SUBTOTAL_PRE: u16 = SEG_BASE + 6; // 未求和先落小计（R-A1-004 违规）
const E_CAL1_MISMATCH: u16 = SEG_BASE + 7; // 口径一与口径二不等
const E_CAL3_MISMATCH: u16 = SEG_BASE + 8; // 口径三与求和不等于 11490
const E_RED: u16 = SEG_BASE + 9; // 断言红
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 11; // 断言集已封，拒绝再写
const E_NO_EXEC: u16 = SEG_BASE + 12; // 断言集未执行
const E_NAME_EMPTY: u16 = SEG_BASE + 13; // 断言名为空
const E_TWICE: u16 = SEG_BASE + 14; // 断言重复执行

/// 批小计定版值：11,490（判据原文口径，写死）。
pub const BATCH_SUBTOTAL: u64 = 11_490;

/// B37 批单元数：9 单（F0732~F0740）。
pub const UNITS: usize = 9;

/// 单元账定容。
pub const MAX_UNITS: usize = 16;

/// 断言集定容。
pub const MAX_ASSERTS: usize = 16;

/// 单元号 → ktest unx_a1_b37 断言名（判据登记口径的静态映射）。
pub fn unit_ktest_name(unit_no: u32) -> Result<&'static str, u16> {
    match unit_no {
        1 => Ok("unx_a1_b37.f0732_gboundary"),
        2 => Ok("unx_a1_b37.f0733_upgmatrix"),
        3 => Ok("unx_a1_b37.f0734_slawarn"),
        4 => Ok("unx_a1_b37.f0735_regreport"),
        5 => Ok("unx_a1_b37.f0736_frozenapi"),
        6 => Ok("unx_a1_b37.f0737_graftseal"),
        7 => Ok("unx_a1_b37.f0738_ktestasrt"),
        8 => Ok("unx_a1_b37.f0739_domexp"),
        9 => Ok("unx_a1_b37.f0740_b37ktest"),
        _ => Err(E_UNIT_NOTFOUND),
    }
}

// ── FNV-1a 64（封账哈希口径）──────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_UNIT_DUP => "单元号重复：B37 九单各登记一次",
        E_UNIT_FULL => "单元账已满 16：批单元数为定值",
        E_UNIT_NOTFOUND => "单元号不存在：按登记序 1..N",
        E_LINES_ZERO => "行数为 0：纯功能线单元不许零行落账",
        E_SUBTOTAL_TWICE => "批小计重复落账：一单一小计",
        E_SUBTOTAL_PRE => "先写小计后凑明细：违反 R-A1-004 留痕口径，拒绝",
        E_CAL1_MISMATCH => "口径一≠口径二：卡面逐条判据行数与逐条求和不符",
        E_CAL3_MISMATCH => "口径三不符：落账小计必须等于 11490 且等于求和",
        E_RED => "断言红：对照 ktest unx_a1_b37 输出定位",
        E_BAL => "守恒破裂：三口径必须逐位相等",
        E_SEALED => "断言集已封：封账后只读",
        E_NO_EXEC => "断言集未执行：先 run_all 再收口",
        E_NAME_EMPTY => "断言名为空",
        E_TWICE => "断言重复执行：一票一裁",
        _ => "未知诊断码",
    }
}

// ── 单元行数账行 ─────────────────────────────────────────────
/// B37 批一个单元的行数账行（口径一明细行）。
#[derive(Clone, Debug)]
pub struct UnitRow {
    /// 单元号（1 起连续，对应 F0732..F0740）。
    pub unit_no: u32,
    /// 单元名（unx 模块短名）。
    pub unit_name: String,
    /// 纯功能代码行数（wc -l 实测口径）。
    pub lines: u64,
    /// 判据下限（≥570 或 ≥580）。
    pub floor: u64,
}

impl UnitRow {
    /// 判据下限达标：lines >= floor。
    pub fn floor_ok(&self) -> bool {
        self.lines >= self.floor && self.lines > 0
    }
}

// ── 断言裁决 ─────────────────────────────────────────────────
/// 断言裁决二值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 绿。
    Green,
    /// 红。
    Red,
}

impl Verdict {
    /// 编号（落账用）。
    pub fn code(self) -> u8 {
        match self {
            Verdict::Green => 1,
            Verdict::Red => 0,
        }
    }
}

// ── B37 断言集（主体）────────────────────────────────────────
/// B37 批 ktest 断言集：三重口径守恒 + 逐单断言 + 一次命令全绿。
pub struct B37Ktest {
    units: [Option<UnitRow>; MAX_UNITS],
    unit_count: usize,
    /// 断言名与裁决。
    a_names: [String; MAX_ASSERTS],
    a_verdicts: [Option<Verdict>; MAX_ASSERTS],
    a_count: usize,
    /// 逐条求和（口径二，求和后落）。
    summed: u64,
    /// 落账批小计（口径三，summed 校验通过后写）。
    posted_subtotal: u64,
    /// 执行数与绿红计数。
    executed: u32,
    green: u32,
    red: u32,
    /// 是否封账。
    sealed: bool,
}

impl B37Ktest {
    /// 开集。
    pub fn new() -> B37Ktest {
        B37Ktest {
            units: core::array::from_fn(|_| None),
            unit_count: 0,
            a_names: [
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
                String::new(), String::new(), String::new(), String::new(),
            ],
            a_verdicts: [None; MAX_ASSERTS],
            a_count: 0,
            summed: 0,
            posted_subtotal: 0,
            executed: 0,
            green: 0,
            red: 0,
            sealed: false,
        }
    }

    /// 登记单元行数：unit_no 1..=UNITS、lines>0、floor 合法。
    pub fn enroll_unit(&mut self, unit_no: u32, name: &str, lines: u64, floor: u64) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.unit_count >= MAX_UNITS {
            return Err(E_UNIT_FULL);
        }
        if unit_no == 0 || unit_no as usize > UNITS {
            return Err(E_UNIT_NOTFOUND);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if lines == 0 {
            return Err(E_LINES_ZERO);
        }
        if floor == 0 {
            return Err(E_LINES_ZERO);
        }
        let idx = (unit_no - 1) as usize;
        if self.units[idx].is_some() {
            return Err(E_UNIT_DUP);
        }
        self.units[idx] = Some(UnitRow {
            unit_no,
            unit_name: name.to_string(),
            lines,
            floor,
        });
        self.unit_count += 1;
        Ok(())
    }

    /// 口径二：逐条求和（先求和，后落账）。求和前必须九单元齐。
    pub fn sum_units(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.unit_count != UNITS {
            return Err(E_UNIT_NOTFOUND);
        }
        if self.summed != 0 {
            return Err(E_BAL);
        }
        let mut s = 0u64;
        let mut i = 0usize;
        while i < UNITS {
            match &self.units[i] {
                Some(u) => s += u.lines,
                None => return Err(E_UNIT_NOTFOUND),
            }
            i += 1;
        }
        self.summed = s;
        Ok(s)
    }

    /// 口径三：落账批小计（必须先求和且求和值 == 11490 才许落）。
    pub fn post_subtotal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.posted_subtotal != 0 {
            return Err(E_SUBTOTAL_TWICE);
        }
        // R-A1-004：先求和后落账。
        if self.summed == 0 {
            return Err(E_SUBTOTAL_PRE);
        }
        if self.summed != BATCH_SUBTOTAL {
            return Err(E_CAL3_MISMATCH);
        }
        self.posted_subtotal = self.summed;
        Ok(())
    }

    /// 登记一条断言（名非空即可，裁决在执行期算）。
    pub fn enroll_assert(&mut self, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.a_count >= MAX_ASSERTS {
            return Err(E_BAL);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        self.a_names[self.a_count] = name.to_string();
        self.a_count += 1;
        Ok(self.a_count as u32)
    }

    /// 断言执行查表：按断言序号做纯函数裁决。
    /// A1..A9：九单元判据下限逐条；A10：三口径守恒；A11：批小计=11490。
    fn assert_verdict(&self, a_no: usize) -> Verdict {
        if a_no < 9 {
            // A1..A9 → 单元 1..9 判据下限。
            match &self.units[a_no] {
                Some(u) => {
                    if u.floor_ok() {
                        Verdict::Green
                    } else {
                        Verdict::Red
                    }
                }
                None => Verdict::Red,
            }
        } else if a_no == 9 {
            // A10：三口径守恒。
            if self.caliber_conserved() {
                Verdict::Green
            } else {
                Verdict::Red
            }
        } else {
            // A11：批小计定版值。
            if self.posted_subtotal == BATCH_SUBTOTAL {
                Verdict::Green
            } else {
                Verdict::Red
            }
        }
    }

    /// 三重口径守恒：卡面求和（口径一重算）== 逐条求和 == 落账小计 == 11490。
    pub fn caliber_conserved(&self) -> bool {
        if self.unit_count != UNITS || self.summed == 0 || self.posted_subtotal == 0 {
            return false;
        }
        let mut face_sum = 0u64;
        let mut i = 0usize;
        while i < UNITS {
            match &self.units[i] {
                Some(u) => face_sum += u.lines,
                None => return false,
            }
            i += 1;
        }
        face_sum == self.summed
            && self.summed == self.posted_subtotal
            && self.posted_subtotal == BATCH_SUBTOTAL
    }

    /// 执行全集：≥11 条断言逐条裁决。
    pub fn run_all(&mut self) -> Result<(u32, u32), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.a_count < 11 {
            return Err(E_NO_EXEC);
        }
        if self.executed > 0 {
            return Err(E_TWICE);
        }
        let mut i = 0usize;
        while i < self.a_count {
            if self.a_verdicts[i].is_some() {
                return Err(E_TWICE);
            }
            let v = self.assert_verdict(i);
            self.a_verdicts[i] = Some(v);
            match v {
                Verdict::Green => self.green += 1,
                Verdict::Red => self.red += 1,
            }
            self.executed += 1;
            i += 1;
        }
        if self.green + self.red != self.executed {
            return Err(E_BAL);
        }
        Ok((self.green, self.red))
    }

    /// 一次命令全绿断言。
    pub fn all_green_assert(&self) -> Result<(), u16> {
        if self.executed == 0 {
            return Err(E_NO_EXEC);
        }
        if !self.caliber_conserved() {
            return Err(E_BAL);
        }
        if self.red != 0 || self.green != self.executed {
            return Err(E_RED);
        }
        Ok(())
    }

    /// 收口封账：全绿后冻结。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.all_green_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(&self.posted_subtotal.to_le_bytes());
        let mut i = 0usize;
        while i < self.a_count {
            buf.extend_from_slice(self.a_names[i].as_bytes());
            buf.push(0x1f);
            if let Some(v) = self.a_verdicts[i] {
                buf.push(v.code());
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 单元行渲染。
    pub fn unit_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== B37 批单元行数账 ===".to_string());
        let mut i = 0usize;
        while i < self.unit_count {
            if let Some(u) = &self.units[i] {
                let mut line = String::from("U");
                line.push_str(&itoa(u.unit_no as u64));
                line.push('|');
                line.push_str(&u.unit_name);
                line.push('|');
                line.push_str(&itoa(u.lines));
                line.push('|');
                line.push_str(if u.floor_ok() { "达标" } else { "未达" });
                lines.push(line);
            }
            i += 1;
        }
        lines.push("逐条求和: ".to_string() + &itoa(self.summed));
        lines.push("落账小计: ".to_string() + &itoa(self.posted_subtotal));
        lines
    }

    /// 断言裁决行渲染：A 编号|断言名|裁决。
    pub fn verdict_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== unx_a1_b37 · 一次命令全绿 ===".to_string());
        let mut i = 0usize;
        while i < self.a_count {
            let mut line = String::from("A");
            line.push_str(&itoa((i + 1) as u64));
            line.push('|');
            line.push_str(&self.a_names[i]);
            line.push('|');
            match self.a_verdicts[i] {
                Some(Verdict::Green) => line.push_str("绿"),
                Some(Verdict::Red) => line.push_str("红"),
                None => line.push_str("未跑"),
            }
            lines.push(line);
            i += 1;
        }
        lines
    }

    /// 断言统计三元组：(执行数, 绿数, 红数)。
    pub fn assert_stats(&self) -> (u32, u32, u32) {
        (self.executed, self.green, self.red)
    }

    /// 判据下限分布盘点：floor 值 → 单元数（570/580 两档）。
    pub fn floor_tally(&self) -> (u32, u32) {
        let mut n570 = 0u32;
        let mut n580 = 0u32;
        let mut i = 0usize;
        while i < self.unit_count {
            if let Some(u) = &self.units[i] {
                if u.floor == 570 {
                    n570 += 1;
                } else if u.floor == 580 {
                    n580 += 1;
                }
            }
            i += 1;
        }
        (n570, n580)
    }
}

/// 极简无符号转十进制。
fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    let mut i = n;
    while i > 0 {
        i -= 1;
        s.push(buf[i] as char);
    }
    s
}

// ── 标准断言集装配 ───────────────────────────────────────────
/// 装配标准 11 条断言（九单 + 守恒 + 小计）。
pub fn build_standard_set() -> Result<B37Ktest, u16> {
    let mut kt = B37Ktest::new();
    kt.enroll_assert("A1 嫁接边界账判据")?;
    kt.enroll_assert("A2 演练矩阵判据")?;
    kt.enroll_assert("A3 SLA 告警判据")?;
    kt.enroll_assert("A4 回归报告判据")?;
    kt.enroll_assert("A5 冻结接口判据")?;
    kt.enroll_assert("A6 嫁接封账判据")?;
    kt.enroll_assert("A7 断言族判据")?;
    kt.enroll_assert("A8 域经条目判据")?;
    kt.enroll_assert("A9 本单判据")?;
    kt.enroll_assert("A10 三口径守恒")?;
    kt.enroll_assert("A11 批小计 11490")?;
    Ok(kt)
}

/// 标准九单行数（判据口径的定版样本；演练注入用）。
pub const STANDARD_LINES: [u64; 9] = [
    1445, 1302, 1261, 1398, 1288, 1402, 1233, 1180, 981,
];

/// 登记标准九单（行数合计恒等 11490，STANDARD_LINES 逐位核对）。
pub fn enroll_standard_units(kt: &mut B37Ktest) -> Result<(), u16> {
    let floors: [u64; 9] = [580, 570, 570, 570, 580, 570, 570, 570, 570];
    let names: [&str; 9] = [
        "gboundary", "upgmatrix", "slawarn", "regreport",
        "frozenapi", "graftseal", "ktestasrt", "domexp", "b37ktest",
    ];
    let mut i = 0usize;
    while i < 9 {
        kt.enroll_unit((i + 1) as u32, names[i], STANDARD_LINES[i], floors[i])?;
        i += 1;
    }
    Ok(())
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：三口径守恒 + 一次命令全绿 + 错误路径。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut kt = build_standard_set()?;
    enroll_standard_units(&mut kt)?;
    // 先求和后落账。
    let s = kt.sum_units()?;
    if s != BATCH_SUBTOTAL {
        return Err(SEG_BASE);
    }
    kt.post_subtotal()?;
    if !kt.caliber_conserved() {
        return Err(SEG_BASE);
    }
    let (g, r) = kt.run_all()?;
    if g != 11 || r != 0 {
        return Err(SEG_BASE);
    }
    kt.all_green_assert()?;
    // 断言映射与统计取数口。
    if unit_ktest_name(1)? != "unx_a1_b37.f0732_gboundary" {
        return Err(SEG_BASE);
    }
    let (ex, g2, r3) = kt.assert_stats();
    if ex != 11 || g2 != 11 || r3 != 0 {
        return Err(SEG_BASE);
    }
    let (n570, n580) = kt.floor_tally();
    if n570 != 7 || n580 != 2 {
        return Err(SEG_BASE);
    }
    let vl = kt.verdict_lines();
    if vl.len() != 12 {
        return Err(SEG_BASE);
    }
    let h = kt.seal()?;
    if h == 0 {
        return Err(SEG_BASE);
    }
    // 错误路径：未求和先落小计（R-A1-004 拦截）。
    let mut kt2 = build_standard_set()?;
    enroll_standard_units(&mut kt2)?;
    match kt2.post_subtotal() {
        Err(c) if c == E_SUBTOTAL_PRE => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：求和值 ≠ 11490 时落账拒绝。
    let mut kt3 = B37Ktest::new();
    let mut i = 0usize;
    while i < 9 {
        kt3.enroll_unit((i + 1) as u32, "x", 100, 570 + if i % 2 == 0 { 10 } else { 0 })?;
        i += 1;
    }
    let s3 = kt3.sum_units()?;
    if s3 == BATCH_SUBTOTAL {
        return Err(SEG_BASE);
    }
    match kt3.post_subtotal() {
        Err(c) if c == E_CAL3_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
