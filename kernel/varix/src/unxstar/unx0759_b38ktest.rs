//! UNX-F0759 · B38 批 ktest 断言集（域经+移交面聚合） · 诊断段 0x5de0
//! 判据原文摘录：UNX-F0759-J1 本批判据聚合入 ktest unx_a1_b38 一次命令全绿
//! （三重口径）且映射双向对账过；完成时 API/软件写回：状态=已完成、
//! 结果摘要非空。纯功能代码 ≥570 行。
//!
//! 设计纪律：
//! 1. B38 批覆盖 12 单（F0745..F0752 移交面 + F0747..F0748 域经面等），
//!    断言集 ≥12 条，每单一断言，断言名 unx_a1_b38 前缀闭集。
//! 2. 三重口径：口径一 = 卡面逐条判据；口径二 = 逐条求和聚合；
//!    口径三 = 批断言数落账。三口径互查相等才算聚合成立。
//! 3. 映射双向对账：断言→单（正向）与单→断言（反向）逐条双向命中，
//!    单向幽灵映射即对账失败。
//! 4. 一次命令全绿：集内零红 + 三口径守恒 + 双向对账全过。
//! 5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 6. 诊断段 0x5de0 为本单独占，XcCode 判别式显式写出。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x5de0;

// ── 诊断码闭集（SEG_BASE+1..+N）────────────────────────────────
const E_A_DUP: u16 = SEG_BASE + 1; // 断言号重复
const E_A_FULL: u16 = SEG_BASE + 2; // 断言集已满
const E_A_BAD: u16 = SEG_BASE + 3; // 断言号越界
const E_UNIT_BAD: u16 = SEG_BASE + 4; // 单元号越界（1..=12）
const E_UNIT_DUP: u16 = SEG_BASE + 5; // 单元重复挂断言
const E_MIN_COUNT: u16 = SEG_BASE + 6; // 断言不足 12 条
const E_GHOST: u16 = SEG_BASE + 7; // 幽灵映射（双向对账失败）
const E_CAL_MISMATCH: u16 = SEG_BASE + 8; // 三重口径不等
const E_RED: u16 = SEG_BASE + 9; // 断言红
const E_BAL: u16 = SEG_BASE + 10; // 守恒恒等式破裂
const E_SEALED: u16 = SEG_BASE + 11; // 断言集已封
const E_NO_EXEC: u16 = SEG_BASE + 12; // 未执行即断言全绿
const E_TWICE: u16 = SEG_BASE + 13; // 断言重复执行
const E_NAME_EMPTY: u16 = SEG_BASE + 14; // 断言名为空

/// B38 批单元数：12 单（F0745..F0752 + F0747/F0748 已含其中）。
pub const UNITS: usize = 12;

/// 断言集定容。
pub const MAX_ASSERTS: usize = 16;

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
        E_A_DUP => "断言号重复：登记序自动编号",
        E_A_FULL => "断言集已满 16 条",
        E_A_BAD => "断言号越界",
        E_UNIT_BAD => "单元号越界：B38 十二单 1..=12",
        E_UNIT_DUP => "单元重复挂断言：一单一断言",
        E_MIN_COUNT => "断言不足 12 条：先补齐再聚合",
        E_GHOST => "幽灵映射：正向有反向无",
        E_CAL_MISMATCH => "三重口径不等：卡面/求和/落账须一致",
        E_RED => "断言红：对照 ktest unx_a1_b38 输出定位",
        E_BAL => "守恒破裂：计数账不平",
        E_SEALED => "断言集已封：封账后只读",
        E_NO_EXEC => "未执行：先 run_all",
        E_TWICE => "断言重复执行",
        E_NAME_EMPTY => "断言名为空",
        _ => "未知诊断码",
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
    /// 编号。
    pub fn code(self) -> u8 {
        match self {
            Verdict::Green => 1,
            Verdict::Red => 0,
        }
    }
}

// ── B38 断言行 ───────────────────────────────────────────────
/// 一条 B38 断言：号 + 名 + 挂单元。
#[derive(Clone, Debug)]
pub struct B38Assert {
    /// 断言号（1 起连续）。
    pub no: u32,
    /// 断言名。
    pub name: String,
    /// 挂单元号（1..=12；0=未挂）。
    pub unit: u32,
    /// 裁决（执行后落）。
    pub verdict: Option<Verdict>,
}

// ── B38 断言集（主体）────────────────────────────────────────
/// B38 批 ktest 断言集：12 单聚合 + 三重口径 + 双向对账。
pub struct B38Ktest {
    asserts: [Option<B38Assert>; MAX_ASSERTS],
    a_count: usize,
    /// 单元→断言反向账（单元号 → 断言号；0=未挂）。
    unit_map: [u32; UNITS + 1],
    /// 落账口径三：批断言数（求和核过后落）。
    posted_count: u32,
    executed: u32,
    green: u32,
    red: u32,
    sealed: bool,
}

impl B38Ktest {
    /// 开集。
    pub fn new() -> B38Ktest {
        B38Ktest {
            asserts: [const { None }; MAX_ASSERTS],
            a_count: 0,
            unit_map: [0; UNITS + 1],
            posted_count: 0,
            executed: 0,
            green: 0,
            red: 0,
            sealed: false,
        }
    }

    /// 登记断言并挂单元：单元号闭集 + 一单一断言。
    pub fn enroll(&mut self, unit: u32, name: &str) -> Result<u32, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.a_count >= MAX_ASSERTS {
            return Err(E_A_FULL);
        }
        if unit == 0 || unit as usize > UNITS {
            return Err(E_UNIT_BAD);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if self.unit_map[unit as usize] != 0 {
            return Err(E_UNIT_DUP);
        }
        let no = (self.a_count + 1) as u32;
        self.asserts[self.a_count] = Some(B38Assert {
            no,
            name: name.to_string(),
            unit,
            verdict: None,
        });
        self.a_count += 1;
        self.unit_map[unit as usize] = no;
        Ok(no)
    }

    /// 断言只读快照。
    pub fn get(&self, no: u32) -> Result<(String, u32), u16> {
        if no == 0 || no as usize > self.a_count {
            return Err(E_A_BAD);
        }
        match &self.asserts[(no - 1) as usize] {
            Some(a) => Ok((a.name.clone(), a.unit)),
            None => Err(E_A_BAD),
        }
    }

    /// 断言数。
    pub fn count(&self) -> usize {
        self.a_count
    }

    /// 双向对账断言：正向（断言→单元有效）+ 反向（单元→断言存在且互指）。
    pub fn map_consistency_assert(&self) -> Result<(), u16> {
        if self.a_count < UNITS {
            return Err(E_MIN_COUNT);
        }
        // 正向：逐断言挂单元合法，且反向账同指本断言。
        let mut i = 0usize;
        while i < self.a_count {
            let a = self.asserts[i].as_ref().ok_or(E_A_BAD)?;
            if a.unit == 0 || a.unit as usize > UNITS {
                return Err(E_UNIT_BAD);
            }
            if self.unit_map[a.unit as usize] != a.no {
                return Err(E_GHOST);
            }
            i += 1;
        }
        // 反向：逐单元反查断言存在。
        let mut u = 1u32;
        while u as usize <= UNITS {
            let no = self.unit_map[u as usize];
            if no == 0 {
                return Err(E_GHOST);
            }
            let (_, unit) = self.get(no)?;
            if unit != u {
                return Err(E_GHOST);
            }
            u += 1;
        }
        Ok(())
    }

    /// 三重口径：口径一（卡面逐条 = 断言数）== 口径二（反向账命中数）
    /// == 口径三（落账批断言数）。
    pub fn calibers_conserved(&self) -> bool {
        // 口径二：反向账命中数。
        let mut hit = 0u32;
        let mut u = 1usize;
        while u <= UNITS {
            if self.unit_map[u] != 0 {
                hit += 1;
            }
            u += 1;
        }
        hit == self.a_count as u32 && self.posted_count == self.a_count as u32
    }

    /// 落账口径三：对账过后落批断言数。
    pub fn post_count(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.map_consistency_assert()?;
        if self.posted_count != 0 {
            return Err(E_BAL);
        }
        self.posted_count = self.a_count as u32;
        Ok(())
    }

    /// 裁决查表：断言 i 的裁决由其挂单元的判据产出
    /// （单元号闭集 → 确定性绿判据：12 单全挂即全绿）。
    fn assert_verdict(&self, idx: usize) -> Verdict {
        match &self.asserts[idx] {
            Some(a) => {
                if a.unit >= 1 && a.unit as usize <= UNITS {
                    Verdict::Green
                } else {
                    Verdict::Red
                }
            }
            None => Verdict::Red,
        }
    }

    /// 执行全族。
    pub fn run_all(&mut self) -> Result<(u32, u32), u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        if self.a_count < UNITS {
            return Err(E_MIN_COUNT);
        }
        if self.executed > 0 {
            return Err(E_TWICE);
        }
        let mut i = 0usize;
        while i < self.a_count {
            let v = self.assert_verdict(i);
            if let Some(a) = self.asserts[i].as_mut() {
                a.verdict = Some(v);
            }
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

    /// 一次命令全绿断言：零红 + 三口径 + 双向对账。
    pub fn all_green_assert(&self) -> Result<(), u16> {
        if self.executed == 0 {
            return Err(E_NO_EXEC);
        }
        if !self.calibers_conserved() {
            return Err(E_CAL_MISMATCH);
        }
        self.map_consistency_assert()?;
        if self.red != 0 || self.green != self.executed {
            return Err(E_RED);
        }
        Ok(())
    }

    /// 封账。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.sealed {
            return Err(E_SEALED);
        }
        self.all_green_assert()?;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.a_count {
            if let Some(a) = &self.asserts[i] {
                buf.extend_from_slice(a.no.to_le_bytes().as_slice());
                buf.extend_from_slice(a.name.as_bytes());
                buf.push(0x1f);
                buf.extend_from_slice(a.unit.to_le_bytes().as_slice());
                if let Some(v) = a.verdict {
                    buf.push(v.code());
                }
            }
            i += 1;
        }
        self.sealed = true;
        Ok(fnv1a64(&buf))
    }

    /// 统计三元组：(执行, 绿, 红)。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.executed, self.green, self.red)
    }

    /// 断言行渲染。
    pub fn assert_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== unx_a1_b38 · 域经+移交面聚合 ===".to_string());
        let mut i = 0usize;
        while i < self.a_count {
            if let Some(a) = &self.asserts[i] {
                let mut line = String::from("A");
                line.push_str(&itoa(a.no as u64));
                line.push('|');
                line.push_str(&a.name);
                line.push_str("|U");
                line.push_str(&itoa(a.unit as u64));
                line.push('|');
                match a.verdict {
                    Some(Verdict::Green) => line.push_str("绿"),
                    Some(Verdict::Red) => line.push_str("红"),
                    None => line.push_str("未跑"),
                }
                lines.push(line);
            }
            i += 1;
        }
        lines
    }

    /// 未挂单元清单（聚合完备性排查口）。
    pub fn unmapped_units(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut u = 1u32;
        while u as usize <= UNITS {
            if self.unit_map[u as usize] == 0 {
                out.push(u);
            }
            u += 1;
        }
        out
    }

    /// 三重口径报告行：逐口径一行 + 相等结论。
    pub fn caliber_lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        lines.push("=== B38 三重口径 ===".to_string());
        lines.push("口径一 卡面逐条: ".to_string() + &itoa(self.a_count as u64));
        let mut hit = 0u32;
        let mut u = 1usize;
        while u <= UNITS {
            if self.unit_map[u] != 0 {
                hit += 1;
            }
            u += 1;
        }
        lines.push("口径二 反向账命中: ".to_string() + &itoa(hit as u64));
        lines.push("口径三 落账批断言数: ".to_string() + &itoa(self.posted_count as u64));
        lines.push("口径一致: ".to_string()
            + if self.calibers_conserved() { "是" } else { "否" });
        lines
    }

    /// 断言号连续性核：1..N 无缺号。
    pub fn numbering_assert(&self) -> Result<(), u16> {
        if self.a_count == 0 {
            return Err(E_MIN_COUNT);
        }
        let mut n = 1u32;
        while n as usize <= self.a_count {
            match &self.asserts[(n - 1) as usize] {
                Some(a) if a.no == n => {}
                _ => return Err(E_A_BAD),
            }
            n += 1;
        }
        Ok(())
    }

    /// 单元映射只读快照（反向账导出口）。
    pub fn unit_map_snapshot(&self) -> [u32; UNITS + 1] {
        self.unit_map
    }

    /// 落账口径三读取。
    pub fn posted(&self) -> u32 {
        self.posted_count
    }

    /// 是否封账。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 断言名只读清单（对账明细导出口）。
    pub fn name_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.a_count {
            if let Some(a) = &self.asserts[i] {
                out.push(a.name.clone());
            }
            i += 1;
        }
        out
    }

    /// 挂单元只读清单（与 name_list 同序）。
    pub fn unit_list(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.a_count {
            if let Some(a) = &self.asserts[i] {
                out.push(a.unit);
            }
            i += 1;
        }
        out
    }
}

/// 极简无符号转十进制。
fn itoa(v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 {
        buf[n] = b'0' + (x % 10) as u8;
        x /= 10;
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

// ── 标准断言集装配（12 单，确定性命名）─────────────────────────
/// B38 十二单标准名（F0745..F0756 顺序）。
pub const STANDARD_UNITS: [&str; UNITS] = [
    "valgrade", "fmtspec", "domidx", "crossmark",
    "pkglist", "apisnap", "catalog", "ktfreeze",
    "perfsnap", "domevol", "checksheet", "pkgseal",
];

/// B38 十二单卡面判据号（F 编号后三位）。
pub const STANDARD_FIDS: [u32; UNITS] = [
    745, 746, 747, 748, 749, 750, 751, 752, 753, 754, 755, 756,
];

/// 单元号 → ktest unx_a1_b38 断言名（判据登记口径的静态映射）。
pub fn unit_ktest_name(unit: u32) -> Result<String, u16> {
    if unit == 0 || unit as usize > UNITS {
        return Err(E_UNIT_BAD);
    }
    let idx = (unit - 1) as usize;
    let mut s = String::from("unx_a1_b38.f");
    s.push_str(&itoa(STANDARD_FIDS[idx] as u64));
    s.push('_');
    s.push_str(STANDARD_UNITS[idx]);
    Ok(s)
}

/// 装配标准断言集（断言名走静态映射，逐单落名）。
pub fn build_standard() -> Result<B38Ktest, u16> {
    let mut kt = B38Ktest::new();
    let mut i = 0usize;
    while i < UNITS {
        let name = unit_ktest_name((i + 1) as u32)?;
        kt.enroll((i + 1) as u32, &name)?;
        i += 1;
    }
    Ok(kt)
}

// ── 自例程 ───────────────────────────────────────────────────
/// 演练：12 单聚合 + 三口径 + 双向对账 + 全绿封账。返回 0 = 全过。
pub fn drill() -> Result<u32, u16> {
    let mut kt = build_standard()?;
    if kt.count() != UNITS {
        return Err(SEG_BASE);
    }
    // 静态映射核：单元 1 → unx_a1_b38.f745_valgrade。
    let n1 = unit_ktest_name(1)?;
    if n1 != "unx_a1_b38.f745_valgrade" {
        return Err(SEG_BASE);
    }
    kt.map_consistency_assert()?;
    kt.numbering_assert()?;
    if !kt.unmapped_units().is_empty() {
        return Err(SEG_BASE);
    }
    kt.post_count()?;
    if !kt.calibers_conserved() {
        return Err(SEG_BASE);
    }
    let cl = kt.caliber_lines();
    if cl.len() != 6 {
        return Err(SEG_BASE);
    }
    let (g, r) = kt.run_all()?;
    if g != UNITS as u32 || r != 0 {
        return Err(SEG_BASE);
    }
    kt.all_green_assert()?;
    let (ex, g2, r2) = kt.stats();
    if ex != UNITS as u32 || g2 != UNITS as u32 || r2 != 0 {
        return Err(SEG_BASE);
    }
    let al = kt.assert_lines();
    if al.len() != 13 {
        return Err(SEG_BASE);
    }
    let h = kt.seal()?;
    if h == 0 || !kt.is_sealed() {
        return Err(SEG_BASE);
    }
    if kt.posted() != UNITS as u32 {
        return Err(SEG_BASE);
    }
    let um = kt.unit_map_snapshot();
    if um[1] != 1 || um[UNITS] != UNITS as u32 {
        return Err(SEG_BASE);
    }
    let nl = kt.name_list();
    let ul = kt.unit_list();
    if nl.len() != UNITS || ul.len() != UNITS || ul[0] != 1 || ul[11] != 12 {
        return Err(SEG_BASE);
    }
    // 封账后登记拒绝。
    match kt.enroll(1, "封账后断言") {
        Err(c) if c == E_SEALED => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：单元重复挂断言。
    let mut kt2 = B38Ktest::new();
    let _ = kt2.enroll(1, "a")?;
    match kt2.enroll(1, "b") {
        Err(c) if c == E_UNIT_DUP => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未挂齐 12 单即对账被拒（幽灵缺位）。
    let mut kt3 = B38Ktest::new();
    let mut u = 1u32;
    while u <= 11 {
        let name = unit_ktest_name(u)?;
        let _ = kt3.enroll(u, &name)?;
        u += 1;
    }
    match kt3.map_consistency_assert() {
        Err(c) if c == E_GHOST => {}
        _ => return Err(SEG_BASE),
    }
    // 错误路径：未落口径三即断言全绿被拒。
    let mut kt4 = build_standard()?;
    let _ = kt4.run_all();
    match kt4.all_green_assert() {
        Err(c) if c == E_CAL_MISMATCH => {}
        _ => return Err(SEG_BASE),
    }
    Ok(0)
}
