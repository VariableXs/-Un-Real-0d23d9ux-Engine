//! UNX-F50093 字体样本页（×4 档样本入账：西文/CJK/混排）（0x437c0 段）
//!
//! 锚点：UNX-F50093-J1 样本哈希账可复跑；判据锚定 §AI-63 §3。
//!
//! 样本页是字体验收的「可复算证据」：每一档样本先按确定性度量模型排成
//! 字形序列，再对（档位、字序号、字形号、步进）逐项做 FNV-1a 摘要入账。
//! 只要度量模型与样本定义不变，任何机器、任何批次重放账本都必须得到同
//! 一个摘要——这就是「可复跑」的工程含义，而不是截一张图说看起来一样。
//!
//! 四档语义：
//! * Ascii：半角 1024 单位步进，覆盖西文基准；
//! * Latin：扩展拉丁 1100 单位，验证重音字形不塌陷；
//! * Cjk  ：全角 2048 单位，验证双宽推进；
//! * Mixed：西文/拉丁/CJK 三段拼接，验证脚本切换时账本仍守恒。
//!
//! 结构清单：
//! * 档位枚举 [`Tier`] + 样本规格 [`SampleSpec`] / 记录 [`SampleRecord`]；
//! * 账本 [`SampleLedger`]（定容、Collecting→Sealed 状态机、逐条复核）；
//! * 重放器 [`replay`]（同一计划两次重放摘要逐位一致）；
//! * 注入核 [`injection_core`]：7 类坏样本逐类断言具体诊断码。
//!
//! 诊断码闭集：`SEG_BASE + n`，n ∈ [1,9]。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x437c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x437c0;

pub const E_EMPTY: u16 = 1;
pub const E_TIER: u16 = 2;
pub const E_DUP: u16 = 3;
pub const E_CAP: u16 = 4;
pub const E_BADCODE: u16 = 5;
pub const E_ZEROW: u16 = 6;
pub const E_HASH: u16 = 7;
pub const E_NOTFOUND: u16 = 8;
pub const E_ORDER: u16 = 9;

const FNV_PRIME: u64 = 0x1000_0000_01b3;
const FNV_SEED: u64 = 0xcbf2_9ce4_8422_2325;

/// 样本错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleError {
    Empty,
    Tier,
    Duplicate,
    Cap,
    BadCode,
    ZeroAdvance,
    HashMismatch,
    NotFound,
    Order,
}

impl SampleError {
    /// 段内序号（闭集 1..=9）。
    pub fn ordinal(self) -> u16 {
        match self {
            SampleError::Empty => E_EMPTY,
            SampleError::Tier => E_TIER,
            SampleError::Duplicate => E_DUP,
            SampleError::Cap => E_CAP,
            SampleError::BadCode => E_BADCODE,
            SampleError::ZeroAdvance => E_ZEROW,
            SampleError::HashMismatch => E_HASH,
            SampleError::NotFound => E_NOTFOUND,
            SampleError::Order => E_ORDER,
        }
    }

    /// 全仓诊断码。
    pub fn code(self) -> u32 {
        SEG_BASE + self.ordinal() as u32
    }

    /// 一行诊断说明。
    pub fn message(self) -> &'static str {
        match self {
            SampleError::Empty => "sample text empty",
            SampleError::Tier => "tier code unknown",
            SampleError::Duplicate => "sample id already booked",
            SampleError::Cap => "ledger capacity exhausted",
            SampleError::BadCode => "codepoint beyond unicode range",
            SampleError::ZeroAdvance => "codepoint maps to zero advance",
            SampleError::HashMismatch => "sample hash does not recompute",
            SampleError::NotFound => "sample id not in ledger",
            SampleError::Order => "ledger already sealed",
        }
    }
}

/// 四档样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Ascii,
    Latin,
    Cjk,
    Mixed,
}

impl Tier {
    /// 档位码（0..3），账本与摘要都用它而不是名字。
    pub fn code(self) -> u8 {
        match self {
            Tier::Ascii => 0,
            Tier::Latin => 1,
            Tier::Cjk => 2,
            Tier::Mixed => 3,
        }
    }

    /// 码值反解；越界显式失败，不做 clamp。
    pub fn from_code(c: u8) -> Result<Tier, SampleError> {
        match c {
            0 => Ok(Tier::Ascii),
            1 => Ok(Tier::Latin),
            2 => Ok(Tier::Cjk),
            3 => Ok(Tier::Mixed),
            _ => Err(SampleError::Tier),
        }
    }

    /// 一行标签（日志/遥测用）。
    pub fn label(self) -> &'static str {
        match self {
            Tier::Ascii => "ascii",
            Tier::Latin => "latin",
            Tier::Cjk => "cjk",
            Tier::Mixed => "mixed",
        }
    }
}

/// 样本规格：id 在账本内唯一，text 是码点序列（不驻留字符串解码状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleSpec {
    pub id: u16,
    pub tier: Tier,
    pub text: Vec<u32>,
}

/// 入账记录：从规格确定性推出的度量摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleRecord {
    pub id: u16,
    pub tier: u8,
    pub glyphs: u16,
    pub ink: u16,
    pub advance_sum: u32,
    pub hash: u64,
}

/// 账本条目保留原规格，复核时才能「从源头重算」而不是信记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleEntry {
    pub spec: SampleSpec,
    pub record: SampleRecord,
}

/// 账本阶段：Sealed 之后不再收新样本，保证已交付摘要不被追加污染。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerStage {
    Collecting,
    Sealed,
}

impl LedgerStage {
    fn code(self) -> u8 {
        match self {
            LedgerStage::Collecting => 0,
            LedgerStage::Sealed => 1,
        }
    }
}

/// 确定性字形号：纯函数，不依赖字库内容，便于跨机复算。
pub fn glyph_id(cp: u32) -> u16 {
    let h = cp.wrapping_mul(0x9E37_79B1) ^ (cp >> 13);
    ((h >> 8) & 0xFFFF) as u16
}

/// 脚本分类：0 西文、1 扩展拉丁、2 CJK。混排覆盖判据依赖它。
pub fn script_of(cp: u32) -> u8 {
    if cp < 0x80 {
        0
    } else if cp < 0x2E80 {
        1
    } else {
        2
    }
}

/// 确定性步进（字体单位）：半角 1024 / 扩展拉丁 1100 / 全角 2048，
/// 空格记 0 但不允许出现在行首之外（空文本由 [`SampleError::Empty`]
/// 拒绝，NUL 由 [`SampleError::ZeroAdvance`] 拒绝）。
pub fn advance_of(cp: u32) -> Result<u16, SampleError> {
    if cp > 0x10FFFF {
        return Err(SampleError::BadCode);
    }
    if cp == 0 {
        return Err(SampleError::ZeroAdvance);
    }
    if cp == 0x20 {
        return Ok(0);
    }
    if cp >= 0x2E80 {
        Ok(2048)
    } else if cp >= 0x80 {
        Ok(1100)
    } else {
        Ok(1024)
    }
}

fn step(h: u64, v: u64) -> u64 {
    let mut x = v;
    let mut out = h;
    let mut k = 0usize;
    while k < 8 {
        out = (out ^ (x & 0xFF)).wrapping_mul(FNV_PRIME);
        x >>= 8;
        k += 1;
    }
    out
}

/// 从规格推出记录。哈希按 (tier, id, len, 每个码点的 cp/advance/gid) 顺序
/// 折叠——顺序敏感是刻意的，样本页读序变了就该是另一份证据。
pub fn render(spec: &SampleSpec) -> Result<SampleRecord, SampleError> {
    if spec.text.is_empty() {
        return Err(SampleError::Empty);
    }
    let mut hash = step(FNV_SEED, spec.tier.code() as u64);
    hash = step(hash, spec.id as u64);
    hash = step(hash, spec.text.len() as u64);
    let mut advance_sum = 0u32;
    let mut glyphs = 0u16;
    let mut ink = 0u16;
    let mut i = 0usize;
    while let Some(cp) = spec.text.get(i) {
        let adv = advance_of(*cp)?;
        let gid = glyph_id(*cp);
        hash = step(hash, *cp as u64);
        hash = step(hash, adv as u64);
        hash = step(hash, gid as u64);
        advance_sum = advance_sum.saturating_add(adv as u32);
        glyphs = glyphs.saturating_add(1);
        if *cp != 0x20 {
            ink = ink.saturating_add(1);
        }
        i += 1;
    }
    Ok(SampleRecord {
        id: spec.id,
        tier: spec.tier.code(),
        glyphs,
        ink,
        advance_sum,
        hash,
    })
}

/// 样本账本：定容、单写者、逐条可复核。
pub struct SampleLedger {
    pub entries: Vec<SampleEntry>,
    cap: usize,
    stage: LedgerStage,
}

impl SampleLedger {
    /// 新建账本；容量 0 视为无效（首次 book 即 Cap）。
    pub fn new(cap: usize) -> SampleLedger {
        SampleLedger {
            entries: Vec::with_capacity(cap),
            cap,
            stage: LedgerStage::Collecting,
        }
    }

    /// 当前阶段。
    pub fn stage(&self) -> LedgerStage {
        self.stage
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 入账：先查重复与容量，再渲染；任何一步失败都不落条。
    pub fn book(&mut self, spec: SampleSpec) -> Result<SampleRecord, SampleError> {
        if self.stage != LedgerStage::Collecting {
            return Err(SampleError::Order);
        }
        let mut i = 0usize;
        while let Some(e) = self.entries.get(i) {
            if e.spec.id == spec.id {
                return Err(SampleError::Duplicate);
            }
            i += 1;
        }
        if self.entries.len() >= self.cap {
            return Err(SampleError::Cap);
        }
        let record = render(&spec)?;
        self.entries.push(SampleEntry { spec, record });
        Ok(record)
    }

    /// 封账：之后 book 返回 Order，但复核/摘要仍可用。
    pub fn seal(&mut self) {
        self.stage = LedgerStage::Sealed;
    }

    /// 按 id 取条目。
    pub fn find(&self, id: u16) -> Result<&SampleEntry, SampleError> {
        let mut i = 0usize;
        while let Some(e) = self.entries.get(i) {
            if e.spec.id == id {
                return Ok(e);
            }
            i += 1;
        }
        Err(SampleError::NotFound)
    }

    /// 逐条从规格重算，任何字段不符都报 HashMismatch（含被篡改的摘要）。
    pub fn verify(&self) -> Result<(), SampleError> {
        let mut i = 0usize;
        while let Some(e) = self.entries.get(i) {
            let again = render(&e.spec)?;
            if again != e.record {
                return Err(SampleError::HashMismatch);
            }
            i += 1;
        }
        Ok(())
    }

    /// 账本摘要：覆盖 (id, tier, glyphs, ink, advance_sum, hash) 与阶段，
    /// 顺序敏感，因此「同一条目换序」也能被发现。
    pub fn digest(&self) -> u64 {
        let mut h = step(FNV_SEED, self.stage.code() as u64);
        h = step(h, self.entries.len() as u64);
        let mut i = 0usize;
        while let Some(e) = self.entries.get(i) {
            h = step(h, e.record.id as u64);
            h = step(h, e.record.tier as u64);
            h = step(h, e.record.glyphs as u64);
            h = step(h, e.record.ink as u64);
            h = step(h, e.record.advance_sum as u64);
            h = step(h, e.record.hash);
            i += 1;
        }
        h
    }

    /// 汇总（字形数, 步进和），用于守恒判据。
    pub fn totals(&self) -> (u32, u32) {
        let mut glyphs = 0u32;
        let mut adv = 0u32;
        let mut i = 0usize;
        while let Some(e) = self.entries.get(i) {
            glyphs = glyphs.saturating_add(e.record.glyphs as u32);
            adv = adv.saturating_add(e.record.advance_sum);
            i += 1;
        }
        (glyphs, adv)
    }
}

/// 重放整份计划并返回账本摘要；容量取计划长度，保证不因容量失败。
pub fn replay(plan: &[SampleSpec]) -> Result<u64, SampleError> {
    let mut ledger = SampleLedger::new(plan.len());
    let mut i = 0usize;
    while let Some(spec) = plan.get(i) {
        ledger.book(spec.clone())?;
        i += 1;
    }
    ledger.verify()?;
    Ok(ledger.digest())
}

fn push_codes(out: &mut Vec<u32>, codes: &[u16]) {
    let mut i = 0usize;
    while let Some(c) = codes.get(i) {
        out.push(*c as u32);
        i += 1;
    }
}

fn push_ascii(out: &mut Vec<u32>, text: &[u8]) {
    let mut i = 0usize;
    while let Some(b) = text.get(i) {
        out.push(*b as u32);
        i += 1;
    }
}

/// 四档标准计划：每档一段独立文本，混排为三段拼接。
pub fn standard_plan() -> Vec<SampleSpec> {
    let mut ascii = Vec::new();
    push_ascii(&mut ascii, b"Hamburgefonstiv 0123");
    let mut latin = Vec::new();
    push_ascii(&mut latin, b"Latin ");
    push_codes(&mut latin, &[0x00C0, 0x00E9, 0x00CE, 0x00F5, 0x00FC, 0x00DF]);
    let mut cjk = Vec::new();
    push_codes(
        &mut cjk,
        &[0x6C38, 0x8F7D, 0x5B57, 0x4F53, 0x6837, 0x5F20, 0x9875],
    );
    let mut mixed = Vec::new();
    push_ascii(&mut mixed, b"Fnt ");
    push_codes(&mut mixed, &[0x00C0, 0x00E9]);
    push_ascii(&mut mixed, b" ");
    push_codes(&mut mixed, &[0x6C38, 0x8F7D]);
    let mut plan = Vec::new();
    plan.push(SampleSpec {
        id: 1,
        tier: Tier::Ascii,
        text: ascii,
    });
    plan.push(SampleSpec {
        id: 2,
        tier: Tier::Latin,
        text: latin,
    });
    plan.push(SampleSpec {
        id: 3,
        tier: Tier::Cjk,
        text: cjk,
    });
    plan.push(SampleSpec {
        id: 4,
        tier: Tier::Mixed,
        text: mixed,
    });
    plan
}

fn base_spec() -> SampleSpec {
    let mut text = Vec::new();
    push_ascii(&mut text, b"Ab");
    SampleSpec {
        id: 9,
        tier: Tier::Ascii,
        text,
    }
}

/// 7 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    let empty = SampleSpec {
        id: 1,
        tier: Tier::Ascii,
        text: Vec::new(),
    };
    if render(&empty).err() == Some(SampleError::Empty) {
        caught += 1;
    }
    let mut bad = Vec::new();
    bad.push(0x0011_0000u32);
    let bad = SampleSpec {
        id: 2,
        tier: Tier::Ascii,
        text: bad,
    };
    if render(&bad).err() == Some(SampleError::BadCode) {
        caught += 1;
    }
    let mut zero = Vec::new();
    zero.push(0u32);
    let zero = SampleSpec {
        id: 3,
        tier: Tier::Ascii,
        text: zero,
    };
    if render(&zero).err() == Some(SampleError::ZeroAdvance) {
        caught += 1;
    }
    if Tier::from_code(9).err() == Some(SampleError::Tier) {
        caught += 1;
    }
    let mut dup = SampleLedger::new(4);
    if dup.book(base_spec()).is_ok() && dup.book(base_spec()).err() == Some(SampleError::Duplicate) {
        caught += 1;
    }
    let mut cap = SampleLedger::new(1);
    let mut other = base_spec();
    other.id = 10;
    if cap.book(base_spec()).is_ok() && cap.book(other).err() == Some(SampleError::Cap) {
        caught += 1;
    }
    let mut tampered = SampleLedger::new(4);
    if tampered.book(base_spec()).is_ok() {
        if let Some(e) = tampered.entries.get_mut(0) {
            e.record.hash ^= 1;
        }
        if tampered.verify().err() == Some(SampleError::HashMismatch) {
            caught += 1;
        }
    }
    (caught, total)
}

/// 运行 UNX-F50093 的 14 条判据。
pub fn run_unx50093_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50093");
    let plan = standard_plan();

    let mut tiers_ok = plan.len() == 4;
    let mut i = 0usize;
    while let Some(spec) = plan.get(i) {
        if spec.tier.code() != i as u8 {
            tiers_ok = false;
        }
        let mut j = i + 1;
        while let Some(b) = plan.get(j) {
            if b.id == spec.id {
                tiers_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("50093-J1-四档齐全", tiers_ok, if tiers_ok { "" } else { "档位缺失或重复" });

    let mut ledger = SampleLedger::new(8);
    let mut booked = 0usize;
    let mut i = 0usize;
    while let Some(spec) = plan.get(i) {
        if ledger.book(spec.clone()).is_ok() {
            booked += 1;
        }
        i += 1;
    }
    let ok = booked == 4 && ledger.len() == 4 && ledger.verify().is_ok();
    let det = format!("booked={} verify={:?}", booked, ledger.verify());
    s.add_dyn("50093-J1-四档入账", ok, &det);

    let r1 = replay(&plan);
    let r2 = replay(&plan);
    let ok = match (&r1, &r2) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let det = format!("d1={:?} d2={:?}", r1, r2);
    s.add_dyn("50093-J1-哈希账可复跑", ok, &det);

    let ok = match &r1 {
        Ok(d) => ledger.digest() == *d,
        Err(_) => false,
    };
    let det = format!("ledger={} replay={:?}", ledger.digest(), r1);
    s.add_dyn("50093-J1-账本摘要稳定", ok, &det);

    let mut distinct = true;
    let mut i = 0usize;
    let mut hashes: Vec<u64> = Vec::new();
    while let Some(spec) = plan.get(i) {
        if let Ok(rec) = render(spec) {
            let mut j = 0usize;
            while let Some(h) = hashes.get(j) {
                if *h == rec.hash {
                    distinct = false;
                }
                j += 1;
            }
            hashes.push(rec.hash);
        }
        i += 1;
    }
    let ok = distinct && hashes.len() == 4;
    let det = format!("distinct={} n={}", distinct, hashes.len());
    s.add_dyn("50093-J1-档位哈希互异", ok, &det);

    let mut same_text = Vec::new();
    push_ascii(&mut same_text, b"ABC");
    let a = SampleSpec {
        id: 77,
        tier: Tier::Ascii,
        text: same_text.clone(),
    };
    let b = SampleSpec {
        id: 77,
        tier: Tier::Latin,
        text: same_text,
    };
    let ok = match (render(&a), render(&b)) {
        (Ok(ra), Ok(rb)) => ra.hash != rb.hash && ra.glyphs == rb.glyphs,
        _ => false,
    };
    s.add("50093-J1-档位入摘要", ok, if ok { "" } else { "档位未进摘要" });

    let mut seen = [0u8; 3];
    let mut mixed_len = 0usize;
    if let Some(mixed) = plan.get(3) {
        mixed_len = mixed.text.len();
        let mut i = 0usize;
        while let Some(cp) = mixed.text.get(i) {
            let sc = script_of(*cp) as usize;
            if let Some(slot) = seen.get_mut(sc) {
                *slot += 1;
            }
            i += 1;
        }
    }
    let ok = seen[0] > 0 && seen[1] > 0 && seen[2] > 0 && mixed_len > 0;
    let det = format!("scripts={}/{}/{}", seen[0], seen[1], seen[2]);
    s.add_dyn("50093-J1-混排覆盖三脚本", ok, &det);

    let ok = match (advance_of(b'A' as u32), advance_of(0x00E9), advance_of(0x6C38)) {
        (Ok(a1), Ok(l1), Ok(c1)) => c1 > l1 && l1 > a1 && c1 == 2048,
        _ => false,
    };
    s.add("50093-J1-CJK双宽", ok, if ok { "" } else { "步进档位错" });

    let ok = advance_of(0x20) == Ok(0) && advance_of(0x110000).err() == Some(SampleError::BadCode);
    s.add("50093-J1-空档与越界", ok, if ok { "" } else { "边界码错" });

    let (glyphs, adv) = ledger.totals();
    let mut want_g = 0u32;
    let mut want_a = 0u32;
    let mut i = 0usize;
    while let Some(e) = ledger.entries.get(i) {
        want_g += e.record.glyphs as u32;
        want_a += e.record.advance_sum;
        i += 1;
    }
    let ok = glyphs == want_g && adv == want_a && want_g > 0;
    let det = format!("g={}/{} a={}/{}", glyphs, want_g, adv, want_a);
    s.add_dyn("50093-J1-汇总守恒", ok, &det);

    let empty = SampleSpec {
        id: 5,
        tier: Tier::Ascii,
        text: Vec::new(),
    };
    let ok = render(&empty).err() == Some(SampleError::Empty);
    s.add("50093-J1-空样本拒", ok, if ok { "" } else { "空样本入账" });

    let mut dup = SampleLedger::new(4);
    let ok = dup.book(base_spec()).is_ok()
        && dup.book(base_spec()).err() == Some(SampleError::Duplicate);
    s.add("50093-J1-重复必拒", ok, if ok { "" } else { "重复未拒" });

    let mut tiny = SampleLedger::new(1);
    let mut second = base_spec();
    second.id = 11;
    let ok = tiny.book(base_spec()).is_ok() && tiny.book(second).err() == Some(SampleError::Cap);
    s.add("50093-J1-容量必拒", ok, if ok { "" } else { "容量未拒" });

    let mut sealed = SampleLedger::new(4);
    let ok = sealed.book(base_spec()).is_ok();
    sealed.seal();
    let mut extra = base_spec();
    extra.id = 12;
    let ok = ok && sealed.book(extra).err() == Some(SampleError::Order);
    s.add("50093-J1-封账拒写", ok, if ok { "" } else { "封账后仍写" });

    let mut tampered = SampleLedger::new(4);
    let ok = tampered.book(base_spec()).is_ok();
    if let Some(e) = tampered.entries.get_mut(0) {
        e.record.advance_sum = e.record.advance_sum.saturating_add(1);
    }
    let ok = ok && tampered.verify().err() == Some(SampleError::HashMismatch);
    s.add("50093-J1-篡改可检", ok, if ok { "" } else { "篡改未检出" });

    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50093-J1-注入必抓", ok, &det);

    let all = [
        SampleError::Empty,
        SampleError::Tier,
        SampleError::Duplicate,
        SampleError::Cap,
        SampleError::BadCode,
        SampleError::ZeroAdvance,
        SampleError::HashMismatch,
        SampleError::NotFound,
        SampleError::Order,
    ];
    let mut unique = true;
    let mut i = 0usize;
    while let Some(a) = all.get(i) {
        if a.code() < SEG_BASE + 1 || a.code() > SEG_BASE + 9 {
            unique = false;
        }
        let mut j = i + 1;
        while let Some(b) = all.get(j) {
            if a.code() == b.code() {
                unique = false;
            }
            j += 1;
        }
        i += 1;
    }
    let ok = unique;
    s.add("50093-J1-码闭集", ok, if ok { "" } else { "码值越界或重复" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50093_checks_green() {
        let set = run_unx50093_checks();
        assert!(set.all_passed(), "UNX-F50093 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50093_replay_is_stable() {
        let plan = standard_plan();
        let a = replay(&plan);
        let b = replay(&plan);
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "replay failed"),
        }
    }
}
