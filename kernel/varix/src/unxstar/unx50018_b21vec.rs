//! UNX-F50018 B21 批向量账骨架（色彩用例集生成器 + 校验和）（0x42e60 段）
//!
//! 锚点：UNX-F50018-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x42e60`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 7`，
//! 逐项见 [`VecError`]。
//!
//! 批向量账解决的是「用例集合能不能被篡改而无人发现」。生成器按
//! 「渲染意图 × 固定色块」笛卡尔积产出用例，每条用例的期望值来自 B21 共用
//! 内核 `rgb8_to_lab`（同一把尺子），并带一枚 FNV-1a 校验和；落账时**只**
//! 接受校验和自洽的条目，入账后任何字段漂移都能被 [`VectorBook::verify`]
//! 全表重算抓出来。
//!
//! 结构清单：
//! * [`VecCase`]：用例三件套（id / 输入 / 期望值）+ 可复算校验和；
//! * [`LedgerEntry`] + [`VectorBook`]：定容向量账（入账 / 校验 / 指纹）；
//! * [`gen_cases`]：确定性用例集生成器（意图码先过闭集校验）；
//! * [`injection_core`]：6 类坏样本注入核（空集 / 非法意图 / 重复 / 篡改 /
//!   容量满 / 越界字段）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{rgb8_to_lab, Intent, Rgb8};

/// 诊断段（0x42e60 独占 0x20）。
pub const SEG_BASE: u32 = 0x42e60;

const E_EMPTY: u16 = 1;
const E_FULL: u16 = 2;
const E_DUPLICATE: u16 = 3;
const E_BAD_SUM: u16 = 4;
const E_BAD_INTENT: u16 = 5;
const E_RANGE: u16 = 6;
const E_DRIFT: u16 = 7;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    /// 空用例集 / 空账。
    Empty,
    /// 账本容量已满。
    Full,
    /// 用例 id 重复。
    Duplicate,
    /// 校验和与用例内容不符。
    BadSum,
    /// 意图码不在闭集 0..=3。
    BadIntent,
    /// 字段越界（id=0、期望值非有限等）。
    Range,
    /// 全表重算时发现条目漂移。
    Drift,
}

impl VecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Empty => E_EMPTY,
            VecError::Full => E_FULL,
            VecError::Duplicate => E_DUPLICATE,
            VecError::BadSum => E_BAD_SUM,
            VecError::BadIntent => E_BAD_INTENT,
            VecError::Range => E_RANGE,
            VecError::Drift => E_DRIFT,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Empty => "empty case set",
            VecError::Full => "vector book capacity reached",
            VecError::Duplicate => "case id already present",
            VecError::BadSum => "case checksum mismatch",
            VecError::BadIntent => "intent code outside closed set",
            VecError::Range => "case field out of range",
            VecError::Drift => "ledger entry drifted from its checksum",
        }
    }
}

/// FNV-1a 32 位偏移基。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 32 位质数。
pub const FNV_PRIME: u32 = 0x0100_0193;
/// 单本账容量上限（48 例 = 4 意图 × 12 色块，留 16 槽给补测例）。
pub const MAX_CASES: usize = 64;

fn fnv_byte(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

/// 逐字节续算 FNV-1a（对外暴露，便于判据独立复算）。
pub fn fnv_bytes(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 定点量化：期望值按 1/1000 取整入校验和。
///
/// 为什么不用 `to_bits()`：`to_bits` 把 libm 末位抖动也算进指纹，跨机器
/// 复算会假报篡改；1e-3 是打印量测的实际分辨率，足够分辨篡改又不受末位噪声影响。
fn quant(v: f32) -> i32 {
    if !v.is_finite() {
        return i32::MIN;
    }
    let s = v * 1000.0;
    if s >= 0.0 {
        (s + 0.5) as i32
    } else {
        (s - 0.5) as i32
    }
}

/// 一条色彩向量用例。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VecCase {
    /// 用例号（1 起，账内唯一）。
    pub id: u16,
    /// 输入 8 位 sRGB。
    pub rgb: Rgb8,
    /// 渲染意图码（0..=3，闭集见 [`Intent`]）。
    pub intent: u8,
    /// 期望 L*（来自 B21 共用内核 `rgb8_to_lab`）。
    pub expect_l: f32,
    /// 期望 C*（同上）。
    pub expect_c: f32,
}

impl VecCase {
    /// 构造一条用例；`intent` 必须先过闭集校验，期望值由调用方从内核取。
    pub fn new(id: u16, rgb: Rgb8, intent: u8, expect_l: f32, expect_c: f32) -> VecCase {
        VecCase {
            id,
            rgb,
            intent,
            expect_l,
            expect_c,
        }
    }

    /// 字段合法性：id 非 0、意图码在闭集内、期望值有限。
    pub fn valid(&self) -> bool {
        self.id != 0
            && self.expect_l.is_finite()
            && self.expect_c.is_finite()
            && Intent::from_code(self.intent).is_ok()
    }

    /// 用例校验和：覆盖 id / 三通道 / 意图 / 两个期望值（量化后大端落字节）。
    ///
    /// 字段串入顺序即声明顺序，逐字节喂入不做跳步——这样任何一字节漂移都会
    /// 改变结果，判据才能用「五个变体五枚不同指纹」证明覆盖完整。
    pub fn checksum(&self) -> u32 {
        let mut h = FNV_OFFSET;
        h = fnv_byte(h, (self.id >> 8) as u8);
        h = fnv_byte(h, self.id as u8);
        h = fnv_byte(h, self.rgb.r);
        h = fnv_byte(h, self.rgb.g);
        h = fnv_byte(h, self.rgb.b);
        h = fnv_byte(h, self.intent);
        let l = quant(self.expect_l);
        h = fnv_byte(h, (l >> 24) as u8);
        h = fnv_byte(h, (l >> 16) as u8);
        h = fnv_byte(h, (l >> 8) as u8);
        h = fnv_byte(h, l as u8);
        let c = quant(self.expect_c);
        h = fnv_byte(h, (c >> 24) as u8);
        h = fnv_byte(h, (c >> 16) as u8);
        h = fnv_byte(h, (c >> 8) as u8);
        h = fnv_byte(h, c as u8);
        h
    }

    /// 一行可读描述（落账日志用）。
    pub fn describe(&self) -> String {
        let mut out = String::from("case#");
        out.push_str(&self.id.to_string());
        out.push_str(&format!(
            " rgb({},{},{}) i{} L{:.3} C{:.3}",
            self.rgb.r, self.rgb.g, self.rgb.b, self.intent, self.expect_l, self.expect_c
        ));
        out
    }
}

/// 账内一条记录：用例本体 + 入账时算出的校验和 + 入账序号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgerEntry {
    /// 用例本体。
    pub case: VecCase,
    /// 入账校验和（必须等于 `case.checksum()`）。
    pub sum: u32,
    /// 入账序号（0 起，单调递增；指纹按它排序）。
    pub seq: u16,
}

/// 定容向量账。入账即校验，校验不过不入账（宁缺不假）。
pub struct VectorBook {
    entries: Vec<LedgerEntry>,
    capacity: usize,
    next_seq: u16,
    rejected: u32,
}

impl VectorBook {
    /// 建空账；容量为 0 时按 1 处理（空容器的意义在调用方）。
    pub fn new(capacity: usize) -> VectorBook {
        let cap = if capacity == 0 { 1 } else { capacity.min(1024) };
        VectorBook {
            entries: Vec::with_capacity(cap),
            capacity: cap,
            next_seq: 0,
            rejected: 0,
        }
    }

    fn reject<T>(&mut self, e: VecError) -> Result<T, VecError> {
        self.rejected = self.rejected.saturating_add(1);
        Err(e)
    }

    /// 按 id 找条目下标。
    pub fn find(&self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) if e.case.id == id => return Some(i),
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// 入账：字段合法 + id 唯一 + 容量未满，校验和由账本自己算。
    pub fn insert(&mut self, case: VecCase) -> Result<u32, VecError> {
        if !case.valid() {
            return self.reject(VecError::Range);
        }
        if self.entries.len() >= self.capacity {
            return self.reject(VecError::Full);
        }
        if self.find(case.id).is_some() {
            return self.reject(VecError::Duplicate);
        }
        let sum = case.checksum();
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(LedgerEntry { case, sum, seq });
        Ok(sum)
    }

    /// 带「声称校验和」入账：声称值与自算值不符即拒（防上游伪造）。
    pub fn insert_claimed(&mut self, case: VecCase, claimed: u32) -> Result<u32, VecError> {
        let real = case.checksum();
        if claimed != real {
            return self.reject(VecError::BadSum);
        }
        self.insert(case)
    }

    /// 全表重算：任一条目漂移即返回 `Drift`，否则返回通过条数。
    pub fn verify(&self) -> Result<usize, VecError> {
        let mut i = 0usize;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.case.checksum() != e.sum {
                        return Err(VecError::Drift);
                    }
                }
                None => return Err(VecError::Drift),
            }
            i += 1;
        }
        Ok(self.entries.len())
    }

    /// 账本指纹：按入账序把每条校验和串进 FNV。顺序敏感——换序即换指纹。
    pub fn digest(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fnv_byte(h, (e.case.id >> 8) as u8);
                h = fnv_byte(h, e.case.id as u8);
                h = fnv_byte(h, (e.sum >> 24) as u8);
                h = fnv_byte(h, (e.sum >> 16) as u8);
                h = fnv_byte(h, (e.sum >> 8) as u8);
                h = fnv_byte(h, e.sum as u8);
            }
            i += 1;
        }
        h
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 容量。
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 累计被拒次数（显性记账）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取第 `i` 条（越界返回 `None`，不 panic）。
    pub fn entry(&self, i: usize) -> Option<&LedgerEntry> {
        self.entries.get(i)
    }

    /// 按 id 取用例本体。
    pub fn case(&self, id: u16) -> Option<&VecCase> {
        match self.find(id) {
            Some(i) => self.entries.get(i).map(|e| &e.case),
            None => None,
        }
    }

    /// 导出全部用例（按入账序），供下游回归器消费。
    pub fn cases(&self) -> Vec<VecCase> {
        let mut out = Vec::with_capacity(self.entries.len());
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                out.push(e.case);
            }
            i += 1;
        }
        out
    }
}

/// 生成器的固定色块集：黑 / 白 / 三原色 / 二次色 / 四级灰（共 12 块）。
pub const PATCHES: [Rgb8; 12] = [
    Rgb8::new(0, 0, 0),
    Rgb8::new(255, 255, 255),
    Rgb8::new(255, 0, 0),
    Rgb8::new(0, 255, 0),
    Rgb8::new(0, 0, 255),
    Rgb8::new(255, 255, 0),
    Rgb8::new(0, 255, 255),
    Rgb8::new(255, 0, 255),
    Rgb8::new(64, 64, 64),
    Rgb8::new(128, 128, 128),
    Rgb8::new(192, 192, 192),
    Rgb8::new(32, 96, 224),
];

/// 用例集生成器：意图 × 色块笛卡尔积。
///
/// 顺序契约：外层意图（按入参顺序）、内层色块（按 [`PATCHES`] 顺序），
/// id 从 1 连续递增——同样的入参两次生成逐项一致（判据依赖这一点）。
pub fn gen_cases(intents: &[u8]) -> Result<Vec<VecCase>, VecError> {
    if intents.is_empty() {
        return Err(VecError::Empty);
    }
    // 意图码先过闭集校验：不认识就整体失败，不静默跳过（跳过会让下游
    // 以为「四档都测了」，实际只测了三档）。
    let mut i = 0usize;
    while i < intents.len() {
        match intents.get(i) {
            Some(c) => {
                if Intent::from_code(*c).is_err() {
                    return Err(VecError::BadIntent);
                }
            }
            None => return Err(VecError::BadIntent),
        }
        i += 1;
    }
    let mut out = Vec::with_capacity(intents.len() * PATCHES.len());
    let mut next_id: u16 = 1;
    let mut k = 0usize;
    while k < intents.len() {
        let code = match intents.get(k) {
            Some(c) => *c,
            None => return Err(VecError::BadIntent),
        };
        let mut p = 0usize;
        while p < PATCHES.len() {
            let rgb = match PATCHES.get(p) {
                Some(c) => *c,
                None => return Err(VecError::Range),
            };
            let lab = rgb8_to_lab(rgb);
            out.push(VecCase::new(next_id, rgb, code, lab.l, lab.chroma()));
            next_id = next_id.saturating_add(1);
            p += 1;
        }
        k += 1;
    }
    Ok(out)
}

/// 把一整套用例灌入账本；任一条被拒即整体失败（不留半本账）。
pub fn fill_book(book: &mut VectorBook, cases: &[VecCase]) -> Result<usize, VecError> {
    if cases.is_empty() {
        return Err(VecError::Empty);
    }
    let mut i = 0usize;
    while i < cases.len() {
        match cases.get(i) {
            Some(c) => {
                book.insert(*c)?;
            }
            None => return Err(VecError::Range),
        }
        i += 1;
    }
    Ok(book.len())
}

/// 造一本标准账：四档意图 × 12 色块 = 48 例。
pub fn standard_book() -> Result<VectorBook, VecError> {
    let intents = [0u8, 1, 2, 3];
    let cases = gen_cases(&intents)?;
    let mut book = VectorBook::new(MAX_CASES);
    fill_book(&mut book, &cases)?;
    Ok(book)
}

/// 注入核：6 类坏样本，逐类断言被显式拒绝。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 6usize;

    // 1) 空意图集
    if gen_cases(&[]).err() == Some(VecError::Empty) {
        caught += 1;
    }

    // 2) 意图码越出闭集（4 是合法码的上界外）
    if gen_cases(&[0u8, 4]).err() == Some(VecError::BadIntent) {
        caught += 1;
    }

    // 3) 重复 id
    let mut b = VectorBook::new(4);
    let c1 = VecCase::new(1, Rgb8::new(1, 2, 3), 0, 50.0, 10.0);
    let c2 = VecCase::new(1, Rgb8::new(4, 5, 6), 1, 40.0, 12.0);
    let first_ok = b.insert(c1).is_ok();
    if first_ok && b.insert(c2).err() == Some(VecError::Duplicate) && b.len() == 1 {
        caught += 1;
    }

    // 4) 篡改：声称校验和与实际差一位
    let mut b2 = VectorBook::new(4);
    let c3 = VecCase::new(2, Rgb8::new(9, 9, 9), 2, 33.0, 0.5);
    if b2.insert_claimed(c3, c3.checksum().wrapping_add(1)).err() == Some(VecError::BadSum) {
        caught += 1;
    }

    // 5) 容量满
    let mut b3 = VectorBook::new(1);
    let a = VecCase::new(3, Rgb8::new(10, 20, 30), 0, 20.0, 5.0);
    let bb = VecCase::new(4, Rgb8::new(10, 20, 31), 0, 20.0, 5.0);
    let ok_a = b3.insert(a).is_ok();
    if ok_a && b3.insert(bb).err() == Some(VecError::Full) {
        caught += 1;
    }

    // 6) 字段越界：id=0
    let mut b4 = VectorBook::new(4);
    let bad = VecCase::new(0, Rgb8::new(1, 1, 1), 0, 10.0, 1.0);
    if b4.insert(bad).err() == Some(VecError::Range) {
        caught += 1;
    }

    (caught, total)
}

/// UNX-F50018 自检：12 条判据。
pub fn run_unx50018_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50018");
    let intents = [0u8, 1, 2, 3];

    // J1-1 生成器规模与 id 连续：4 意图 × 12 色块 = 48，id 1..=48 无洞。
    let cases = match gen_cases(&intents) {
        Ok(v) => v,
        Err(_) => Vec::new(),
    };
    let mut ids_ok = cases.len() == 48;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            if c.id as usize != i + 1 {
                ids_ok = false;
            }
        }
        i += 1;
    }
    let ok = ids_ok;
    let det = format!("n={}", cases.len());
    s.add_dyn("50018-J1-生成规模", ok, &det);

    // J1-2 生成器与 50001 同源：每条期望值都能用内核重算出同一定点值。
    let mut same_source = cases.len() == 48;
    let mut k = 0usize;
    while k < cases.len() {
        if let Some(c) = cases.get(k) {
            let lab = rgb8_to_lab(c.rgb);
            if fabsf(lab.l - c.expect_l) > 1e-3 || fabsf(lab.chroma() - c.expect_c) > 1e-3 {
                same_source = false;
            }
        }
        k += 1;
    }
    s.add("50018-J1-期望同源", same_source, if same_source { "" } else { "期望值与内核不一致" });

    // J1-3 生成器确定性：同入参两次逐项相等。
    let again = gen_cases(&intents).ok();
    let ok = match (&again, cases.is_empty()) {
        (Some(v), false) => *v == cases,
        _ => false,
    };
    s.add("50018-J1-生成幂等", ok, if ok { "" } else { "两次生成不一致" });

    // J1-4 校验和覆盖每个字段：改 id / rgb / intent / 期望值各得不同指纹。
    let base = VecCase::new(7, Rgb8::new(11, 22, 33), 1, 44.0, 12.5);
    let m_id = VecCase::new(8, Rgb8::new(11, 22, 33), 1, 44.0, 12.5);
    let m_rgb = VecCase::new(7, Rgb8::new(11, 22, 34), 1, 44.0, 12.5);
    let m_int = VecCase::new(7, Rgb8::new(11, 22, 33), 2, 44.0, 12.5);
    let m_val = VecCase::new(7, Rgb8::new(11, 22, 33), 1, 44.5, 12.5);
    let sums = [base.checksum(), m_id.checksum(), m_rgb.checksum(), m_int.checksum(), m_val.checksum()];
    let mut distinct = true;
    let mut a = 0usize;
    while a < sums.len() {
        let mut b = a + 1;
        while b < sums.len() {
            if let (Some(x), Some(y)) = (sums.get(a), sums.get(b)) {
                if x == y {
                    distinct = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    let ok = distinct && sums.first().map(|x| *x) != Some(FNV_OFFSET);
    s.add("50018-J1-校验和分敏感", ok, if ok { "" } else { "字段未进指纹" });

    // J1-5 入账校验和自洽：返回的 sum 等于用例自算值，seq 从 0 递增。
    let mut book = VectorBook::new(MAX_CASES);
    let filled = fill_book(&mut book, &cases);
    let ok = match filled {
        Ok(n) => n == 48,
        Err(_) => false,
    } && book.entry(0).map(|e| e.seq) == Some(0)
        && book.entry(47).map(|e| e.seq) == Some(47)
        && book.entry(0).map(|e| e.sum) == cases.first().map(|c| c.checksum());
    let det = format!("n={} dg={:#010x}", book.len(), book.digest());
    s.add_dyn("50018-J1-落账自洽", ok, &det);

    // J1-6 重复 id 必拒，账长不变。
    let len_before = book.len();
    let dup = VecCase::new(1, Rgb8::new(0, 0, 0), 0, 0.0, 0.0);
    let ok = book.insert(dup).err() == Some(VecError::Duplicate) && book.len() == len_before;
    s.add("50018-J1-防重复入账", ok, if ok { "" } else { "重复 id 进了账" });

    // J1-7 篡改必拒：同 id 改期望值 + 冒用旧校验和。
    let forged_sum = match book.case(2) {
        Some(c) => c.checksum(),
        None => 0,
    };
    let tampered = VecCase::new(2, Rgb8::new(255, 0, 0), 0, 99.0, 99.0);
    let ok = book.insert_claimed(tampered, forged_sum).err() == Some(VecError::BadSum)
        && book.len() == len_before
        && book.rejected() >= 1;
    s.add("50018-J1-篡改必拒", ok, if ok { "" } else { "篡改条目被接受" });

    // J1-8 全表重算通过，且逐条 sum 与自算一致（不是只看总数）。
    let mut per_entry = true;
    let mut j = 0usize;
    while j < book.len() {
        match book.entry(j) {
            Some(e) => {
                if e.case.checksum() != e.sum {
                    per_entry = false;
                }
            }
            None => per_entry = false,
        }
        j += 1;
    }
    let verified = book.verify();
    let ok = per_entry && verified == Ok(48);
    s.add("50018-J1-全表可复算", ok, if ok { "" } else { "重算与入账不一致" });

    // J1-9 容量满必拒（定容账不得被撑破）。
    let mut small = VectorBook::new(2);
    let a = VecCase::new(10, Rgb8::new(1, 2, 3), 0, 12.0, 3.0);
    let b = VecCase::new(11, Rgb8::new(4, 5, 6), 0, 13.0, 4.0);
    let c = VecCase::new(12, Rgb8::new(7, 8, 9), 0, 14.0, 5.0);
    let ok = small.insert(a).is_ok()
        && small.insert(b).is_ok()
        && small.insert(c).err() == Some(VecError::Full)
        && small.len() == small.capacity();
    let det = format!("len={} cap={}", small.len(), small.capacity());
    s.add_dyn("50018-J1-定容不破", ok, &det);

    // J1-10 指纹对顺序敏感、对重复构造稳定。
    let book2 = match standard_book() {
        Ok(v) => v,
        Err(_) => VectorBook::new(1),
    };
    let mut rev = VectorBook::new(MAX_CASES);
    let mut r = cases.len();
    while r > 0 {
        if let Some(c) = cases.get(r - 1) {
            let _ = rev.insert(*c);
        }
        r -= 1;
    }
    let ok = book2.len() == 48 && book2.digest() == book.digest() && rev.digest() != book.digest();
    let det = format!("d1={:#010x} d2={:#010x}", book.digest(), rev.digest());
    s.add_dyn("50018-J1-指纹有序", ok, &det);

    // J1-11 空入参显式失败：空意图集 Empty、空用例集灌账 Empty、空账 verify=0。
    let empty_book = VectorBook::new(4);
    let ok = gen_cases(&[0u8; 0]).err() == Some(VecError::Empty)
        && fill_book(&mut VectorBook::new(4), &[]).err() == Some(VecError::Empty)
        && empty_book.verify() == Ok(0)
        && empty_book.is_empty();
    s.add("50018-J1-空入参显式", ok, if ok { "" } else { "空入参被静默接受" });

    // J1-12 六类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50018-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50018_checks_green() {
        let set = run_unx50018_checks();
        assert!(set.all_passed(), "UNX-F50018 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50018_generator_deterministic() {
        let a = gen_cases(&[0u8, 1, 2, 3]);
        let b = gen_cases(&[0u8, 1, 2, 3]);
        assert!(a.is_ok() && b.is_ok());
        if let (Ok(x), Ok(y)) = (a, b) {
            assert_eq!(x, y);
            assert_eq!(x.len(), 48);
        }
    }

    #[test]
    fn unx50018_injection_full() {
        let (caught, total) = injection_core();
        assert_eq!(caught, total);
        assert_eq!(total, 6);
    }
}
