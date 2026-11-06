//! UNX-F50078 B24 批向量账骨架（纸张用例集生成器+校验和）（0x435e0 段）
//!
//! 锚点：UNX-F50078-J1 用例带校验和落账；判据锚定 §AI-63 §5。
//!
//! 数据段：`SEG_BASE = 0x435e0`；诊断码闭集 `SEG_BASE + 1..=SEG_BASE + 10`，
//! 逐项见 [`VecError`]。
//!
//! 向量账不是一张手抄清单，而是**生成器 + 校验和 + 只读收口**三段：
//!
//! ```text
//! CaseKind × 尺寸码 × 模式码 --generate--> [CaseSpec;108]
//!         |                                     |
//!         |                                     v
//!         +--case_checksum(FNV-1a)--> VectorLedger.append（逐条落校验和）
//!                                               |
//!                                               v
//!                               seal(): 条数==108 且 折叠==canonical_fold
//! ```
//!
//! 关键纪律：
//! * 生成器无随机源：同一次构建两次调用逐字段一致（可复跑）；
//! * 校验和按**规范字节序**（id/kind/a/b/expect 九字节）计算，字段一改立刻失配；
//! * 账本只追加，封存后拒绝任何写入；封存时用生成器折叠值做整账比对，
//!   单独篡改某条即使重算了单条校验和，也过不了整账折叠。
//!
//! 结构清单：
//! * [`CaseSpec`] / [`CaseKind`]：用例与族分类；
//! * [`VectorLedger`]：定容追加账（append / verify / fold / seal）；
//! * [`VecEntry`]：带校验和的落账条目；
//! * [`injection_core`]：10 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x435e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x435e0;

const E_KIND: u16 = 1;
const E_ID: u16 = 2;
const E_PARAM: u16 = 3;
const E_EXPECT: u16 = 4;
const E_DUPLICATE: u16 = 5;
const E_CAPACITY: u16 = 6;
const E_CHECKSUM: u16 = 7;
const E_INDEX: u16 = 8;
const E_INCOMPLETE: u16 = 9;
const E_SEALED: u16 = 10;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecError {
    /// 族编码不在 0..=5。
    Kind,
    /// 用例号不在本批区间。
    Id,
    /// 尺寸码 / 模式码越界。
    Param,
    /// 期望诊断码不在纸张段窗口。
    Expect,
    /// 用例号重复。
    Duplicate,
    /// 账本已满。
    Capacity,
    /// 校验和不匹配。
    Checksum,
    /// 下标越界。
    Index,
    /// 条数不足，不能收口。
    Incomplete,
    /// 已封存，只读。
    Sealed,
}

impl VecError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            VecError::Kind => E_KIND,
            VecError::Id => E_ID,
            VecError::Param => E_PARAM,
            VecError::Expect => E_EXPECT,
            VecError::Duplicate => E_DUPLICATE,
            VecError::Capacity => E_CAPACITY,
            VecError::Checksum => E_CHECKSUM,
            VecError::Index => E_INDEX,
            VecError::Incomplete => E_INCOMPLETE,
            VecError::Sealed => E_SEALED,
        };
        SEG_BASE + n as u32
    }

    /// 一行英文诊断。
    pub fn message(self) -> &'static str {
        match self {
            VecError::Kind => "case kind code outside 0..=5",
            VecError::Id => "case id outside batch window",
            VecError::Param => "size/mode code outside grid",
            VecError::Expect => "expected diagnostic code outside paper window",
            VecError::Duplicate => "case id already in ledger",
            VecError::Capacity => "vector ledger full",
            VecError::Checksum => "case checksum mismatch",
            VecError::Index => "entry index outside ledger",
            VecError::Incomplete => "case count below batch expectation",
            VecError::Sealed => "vector ledger sealed",
        }
    }
}

/// 纸张用例族数（双面 / 纸盒 / 介质 / 出纸口 / 尺寸 / 旋转）。
pub const KINDS: usize = 6;
/// 单批用例数：6 族 × 6 尺寸码 × 3 模式码 = 108。
pub const EXPECTED_CASES: usize = 108;
/// 账本容量（108 + 20 条重跑余量）。
pub const MAX_CASES: usize = 128;
/// 用例号窗口（B24 段内自用，避免与真机样本号撞车）。
pub const CASE_ID_MIN: u16 = 1000;
/// 用例号窗口上界。
pub const CASE_ID_MAX: u16 = 1999;
/// 尺寸码上界（0..6）。
pub const PARAM_A_MAX: u8 = 6;
/// 模式码上界（0..3）。
pub const PARAM_B_MAX: u8 = 3;
/// 期望诊断码窗口：B24 纸张诊断族 0x43500..=0x435ff。
pub const EXPECT_MIN: u32 = 0x43500;
/// 期望诊断码窗口上界。
pub const EXPECT_MAX: u32 = 0x435ff;
/// FNV-1a 32 位偏移基（RFC 推荐值，非零保证空集折叠可区分）。
pub const FNV_OFFSET: u32 = 0x811c_9dc5;
/// FNV-1a 32 位质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 用例族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 双面装订组合。
    Duplex,
    /// 纸盒选择。
    Tray,
    /// 介质类型。
    Media,
    /// 出纸口 / 分页器。
    Bin,
    /// 纸张尺寸。
    Size,
    /// 旋转与装订方向。
    Rotate,
}

impl CaseKind {
    /// 编码 → 族。非法编码返回 None（不夹带默认族）。
    pub fn from_code(c: u8) -> Option<CaseKind> {
        match c {
            0 => Some(CaseKind::Duplex),
            1 => Some(CaseKind::Tray),
            2 => Some(CaseKind::Media),
            3 => Some(CaseKind::Bin),
            4 => Some(CaseKind::Size),
            5 => Some(CaseKind::Rotate),
            _ => None,
        }
    }

    /// 族 → 编码。
    pub fn code(self) -> u8 {
        match self {
            CaseKind::Duplex => 0,
            CaseKind::Tray => 1,
            CaseKind::Media => 2,
            CaseKind::Bin => 3,
            CaseKind::Size => 4,
            CaseKind::Rotate => 5,
        }
    }

    /// 短名（用于报告与日志，不申请内存）。
    pub fn name(self) -> &'static str {
        match self {
            CaseKind::Duplex => "duplex",
            CaseKind::Tray => "tray",
            CaseKind::Media => "media",
            CaseKind::Bin => "bin",
            CaseKind::Size => "size",
            CaseKind::Rotate => "rotate",
        }
    }
}

/// 一条纸张用例。`a` 是尺寸码，`b` 是模式码，`expect` 是期望诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseSpec {
    /// 用例号。
    pub id: u16,
    /// 族。
    pub kind: CaseKind,
    /// 尺寸码 0..6。
    pub a: u8,
    /// 模式码 0..3。
    pub b: u8,
    /// 期望诊断码。
    pub expect: u32,
}

/// 带校验和的落账条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VecEntry {
    /// 用例本体。
    pub spec: CaseSpec,
    /// 落账时的 FNV-1a 校验和。
    pub checksum: u32,
}

fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ b as u32).wrapping_mul(FNV_PRIME)
}

fn fnv_bytes(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h = fnv_step(h, *b);
        }
        i += 1;
    }
    h
}

/// 规范字节序：id(2) kind(1) a(1) b(1) expect:4 字节，共 9 字节。
pub fn canonical_bytes(c: &CaseSpec) -> [u8; 9] {
    [
        (c.id >> 8) as u8,
        c.id as u8,
        c.kind.code(),
        c.a,
        c.b,
        (c.expect >> 24) as u8,
        (c.expect >> 16) as u8,
        (c.expect >> 8) as u8,
        c.expect as u8,
    ]
}

/// 单条用例校验和（FNV-1a 32）。
pub fn case_checksum(c: &CaseSpec) -> u32 {
    fnv_bytes(FNV_OFFSET, &canonical_bytes(c))
}

/// 从族编码、参数、期望码组装一条用例；`kind` 非法时显式失败。
pub fn make_case(kind_code: u8, id: u16, a: u8, b: u8, expect: u32) -> Result<CaseSpec, VecError> {
    let kind = match CaseKind::from_code(kind_code) {
        Some(k) => k,
        None => return Err(VecError::Kind),
    };
    if id < CASE_ID_MIN || id > CASE_ID_MAX {
        return Err(VecError::Id);
    }
    if a >= PARAM_A_MAX || b >= PARAM_B_MAX {
        return Err(VecError::Param);
    }
    if expect < EXPECT_MIN || expect > EXPECT_MAX {
        return Err(VecError::Expect);
    }
    Ok(CaseSpec { id, kind, a, b, expect })
}

/// 生成纸张用例集：6 族 × 6 尺寸码 × 3 模式码，顺序固定、编号连续。
pub fn generate_paper_cases() -> Vec<CaseSpec> {
    let mut out = Vec::with_capacity(EXPECTED_CASES);
    let mut k = 0u8;
    while k < KINDS as u8 {
        let kind = match CaseKind::from_code(k) {
            Some(v) => v,
            None => CaseKind::Duplex,
        };
        let mut a = 0u8;
        while a < PARAM_A_MAX {
            let mut b = 0u8;
            while b < PARAM_B_MAX {
                let id = CASE_ID_MIN + (k as u16) * 18 + (a as u16) * 3 + b as u16;
                // 期望码按族与尺寸码派生，落在纸张诊断窗口内：0x43500 + k*0x10 + a。
                let expect = EXPECT_MIN + (k as u32) * 0x10 + a as u32;
                out.push(CaseSpec { id, kind, a, b, expect });
                b += 1;
            }
            a += 1;
        }
        k += 1;
    }
    out
}

/// 生成器整账折叠值：作为封存时的权威比对基准。
pub fn canonical_fold() -> u32 {
    let cases = generate_paper_cases();
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            let bytes = canonical_bytes(c);
            h = fnv_bytes(h, &bytes);
        }
        i += 1;
    }
    h
}

/// 用例向量账：只追加，封存后只读。
#[derive(Clone, Debug)]
pub struct VectorLedger {
    entries: Vec<VecEntry>,
    seen: Vec<u16>,
    sealed: bool,
}

impl VectorLedger {
    /// 空账。
    pub fn new() -> VectorLedger {
        VectorLedger {
            entries: Vec::with_capacity(MAX_CASES),
            seen: Vec::with_capacity(MAX_CASES),
            sealed: false,
        }
    }

    /// 按已校验用例追加，返回本次落账的校验和。
    pub fn append(&mut self, c: CaseSpec) -> Result<u32, VecError> {
        if self.sealed {
            return Err(VecError::Sealed);
        }
        if c.id < CASE_ID_MIN || c.id > CASE_ID_MAX {
            return Err(VecError::Id);
        }
        if c.a >= PARAM_A_MAX || c.b >= PARAM_B_MAX {
            return Err(VecError::Param);
        }
        if c.expect < EXPECT_MIN || c.expect > EXPECT_MAX {
            return Err(VecError::Expect);
        }
        if self.entries.len() >= MAX_CASES {
            return Err(VecError::Capacity);
        }
        let mut i = 0usize;
        while i < self.seen.len() {
            if let Some(id) = self.seen.get(i) {
                if *id == c.id {
                    return Err(VecError::Duplicate);
                }
            }
            i += 1;
        }
        let checksum = case_checksum(&c);
        self.entries.push(VecEntry { spec: c, checksum });
        self.seen.push(c.id);
        Ok(checksum)
    }

    /// 原始族编码入口：批量装载器从文本/二进制读到的编码先过这里。
    pub fn append_raw(
        &mut self,
        kind_code: u8,
        id: u16,
        a: u8,
        b: u8,
        expect: u32,
    ) -> Result<u32, VecError> {
        let c = make_case(kind_code, id, a, b, expect)?;
        self.append(c)
    }

    /// 重算并比对第 `idx` 条校验和。
    pub fn verify(&self, idx: usize) -> Result<(), VecError> {
        let e = match self.entries.get(idx) {
            Some(v) => v,
            None => return Err(VecError::Index),
        };
        if case_checksum(&e.spec) != e.checksum {
            return Err(VecError::Checksum);
        }
        Ok(())
    }

    /// 整账折叠（按追加顺序，对顺序敏感）。
    pub fn fold(&self) -> u32 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let bytes = canonical_bytes(&e.spec);
                h = fnv_bytes(h, &bytes);
            }
            i += 1;
        }
        h
    }

    /// 封存：条数必须 108，且整账折叠必须等于生成器折叠值。
    pub fn seal(&mut self) -> Result<u32, VecError> {
        if self.sealed {
            return Err(VecError::Sealed);
        }
        if self.entries.len() != EXPECTED_CASES {
            return Err(VecError::Incomplete);
        }
        let fold = self.fold();
        if fold != canonical_fold() {
            return Err(VecError::Checksum);
        }
        self.sealed = true;
        Ok(fold)
    }

    /// 已落账条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空账判定（`len` 的伴生方法，避免 clippy 提示）。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否已封存。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 读一条落账条目。
    pub fn entry(&self, idx: usize) -> Option<VecEntry> {
        self.entries.get(idx).copied()
    }

    /// 各族条数统计（[duplex, tray, media, bin, size, rotate]）。
    pub fn per_kind(&self) -> [u16; KINDS] {
        let mut cnt = [0u16; KINDS];
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let k = e.spec.kind.code() as usize;
                if k < KINDS {
                    cnt[k] += 1;
                }
            }
            i += 1;
        }
        cnt
    }

    /// 一行报告（结果摘要非空，供软件写回）。
    pub fn report(&self) -> String {
        let k = self.per_kind();
        format!(
            "B24 vectors={} fold={:08x} sealed={} kinds=[{},{},{},{},{},{}]",
            self.entries.len(),
            self.fold(),
            self.sealed as u8,
            k[0], k[1], k[2], k[3], k[4], k[5]
        )
    }
}

impl Default for VectorLedger {
    fn default() -> VectorLedger {
        VectorLedger::new()
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

/// 坏样本注入核：10 类错误必须逐类命中具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 10usize;

    // 1 族编码越界
    if make_case(9, 1000, 0, 0, EXPECT_MIN).err() == Some(VecError::Kind) {
        caught += 1;
    }
    // 2 用例号越界
    if make_case(0, 999, 0, 0, EXPECT_MIN).err() == Some(VecError::Id) {
        caught += 1;
    }
    // 3 尺寸码越界
    if make_case(0, 1000, 9, 0, EXPECT_MIN).err() == Some(VecError::Param) {
        caught += 1;
    }
    // 4 期望码越界
    if make_case(0, 1000, 0, 0, 0).err() == Some(VecError::Expect) {
        caught += 1;
    }
    // 5 重复用例号
    let mut led = VectorLedger::new();
    let case = CaseSpec { id: 1000, kind: CaseKind::Duplex, a: 0, b: 0, expect: EXPECT_MIN };
    let _ = led.append(case);
    if led.append(case).err() == Some(VecError::Duplicate) {
        caught += 1;
    }
    // 6 容量封顶
    let mut led = VectorLedger::new();
    let mut i = 0u16;
    while i < MAX_CASES as u16 {
        let c = CaseSpec { id: CASE_ID_MIN + i, kind: CaseKind::Tray, a: 0, b: 0, expect: EXPECT_MIN };
        let _ = led.append(c);
        i += 1;
    }
    let over = CaseSpec { id: CASE_ID_MAX, kind: CaseKind::Bin, a: 0, b: 0, expect: EXPECT_MIN };
    if led.append(over).err() == Some(VecError::Capacity) {
        caught += 1;
    }
    // 7 单条篡改
    let mut led = VectorLedger::new();
    let _ = led.append(case);
    if let Some(e) = led.entries.get_mut(0) {
        e.spec.a = 1;
    }
    if led.verify(0).err() == Some(VecError::Checksum) {
        caught += 1;
    }
    // 8 下标越界
    let led = VectorLedger::new();
    if led.verify(0).err() == Some(VecError::Index) {
        caught += 1;
    }
    // 9 条数不足收口
    let mut led = VectorLedger::new();
    let _ = led.append(case);
    if led.seal().err() == Some(VecError::Incomplete) {
        caught += 1;
    }
    // 10 封存后追加
    let mut led = match fill_full_ledger() {
        Ok(v) => v,
        Err(_) => VectorLedger::new(),
    };
    let _ = led.seal();
    let extra = CaseSpec { id: CASE_ID_MAX, kind: CaseKind::Rotate, a: 0, b: 0, expect: EXPECT_MIN };
    if led.append(extra).err() == Some(VecError::Sealed) {
        caught += 1;
    }
    (caught, total)
}

/// 用生成器填满 108 条并逐条校验；任一条失败即整体失败。
pub fn fill_full_ledger() -> Result<VectorLedger, VecError> {
    let cases = generate_paper_cases();
    let mut led = VectorLedger::new();
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            led.append(*c)?;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < led.len() {
        led.verify(j)?;
        j += 1;
    }
    Ok(led)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 跑 UNX-F50078 的 13 条判据（含 J1 校验和落账与注入核）。
pub fn run_unx50078_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50078");

    // J1-1 生成器可复跑：两次调用逐字段一致。
    let a = generate_paper_cases();
    let b = generate_paper_cases();
    let same = {
        if a.len() != b.len() {
            false
        } else {
            let mut ok = true;
            let mut i = 0usize;
            while i < a.len() {
                match (a.get(i), b.get(i)) {
                    (Some(x), Some(y)) => {
                        if x != y {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        }
    };
    s.add("50078-J1-生成确定", same, if same { "" } else { "两次生成不一致" });

    // J1-2 规模与编号唯一：108 条，用例号无重复。
    let mut ids: Vec<u16> = Vec::with_capacity(a.len());
    let mut dup = false;
    let mut i = 0usize;
    while i < a.len() {
        if let Some(c) = a.get(i) {
            let mut j = 0usize;
            while j < ids.len() {
                if let Some(x) = ids.get(j) {
                    if *x == c.id {
                        dup = true;
                    }
                }
                j += 1;
            }
            ids.push(c.id);
        }
        i += 1;
    }
    let ok = a.len() == EXPECTED_CASES && !dup;
    let det = format!("n={} dup={}", a.len(), dup as u8);
    s.add_dyn("50078-J1-规模编号", ok, &det);

    // J1-3 用例带校验和落账：每条落账后重算一致，且返回值为同一校验和。
    let filled = fill_full_ledger();
    let ok = match &filled {
        Ok(led) => {
            let mut good = led.len() == EXPECTED_CASES;
            let mut k = 0usize;
            while k < led.len() {
                if let Some(e) = led.entry(k) {
                    if e.checksum != case_checksum(&e.spec) {
                        good = false;
                    }
                }
                k += 1;
            }
            good
        }
        Err(_) => false,
    };
    s.add("50078-J1-校验和落账", ok, if ok { "" } else { "落账校验和与重算不符" });

    let mut led = match filled {
        Ok(v) => v,
        Err(_) => VectorLedger::new(),
    };

    // J1-4 族分布守恒：6 族 × 18 条，合计等于总条数。
    let cnt = led.per_kind();
    let mut sum = 0u16;
    let mut balanced = true;
    let mut z = 0usize;
    while z < KINDS {
        let c = cnt[z];
        sum += c;
        if c != 18 {
            balanced = false;
        }
        z += 1;
    }
    let ok = balanced && sum as usize == led.len();
    let det = format!("per_kind={:?} sum={}", cnt, sum);
    s.add_dyn("50078-J1-族账守恒", ok, &det);

    // J1-5 整账折叠 = 生成器折叠（落账顺序与生成顺序一致）。
    let ok = led.fold() == canonical_fold();
    let det = format!("fold={:08x} canon={:08x}", led.fold(), canonical_fold());
    s.add_dyn("50078-J1-折叠一致", ok, &det);

    // J1-6 封存成功并只读：返回折叠值，随后追加被拒。
    let sealed = led.seal();
    let extra = CaseSpec { id: CASE_ID_MAX, kind: CaseKind::Rotate, a: 0, b: 0, expect: EXPECT_MIN };
    let after = led.append(extra);
    let ok = sealed == Ok(canonical_fold()) && led.is_sealed() && after.err() == Some(VecError::Sealed);
    s.add("50078-J1-封存只读", ok, if ok { "" } else { "封存后仍可写" });

    // J1-7 单条篡改必抓：改一个字段，单条校验和立即失配。
    let mut t = match fill_full_ledger() {
        Ok(v) => v,
        Err(_) => VectorLedger::new(),
    };
    if let Some(e) = t.entries.get_mut(7) {
        e.spec.expect = e.spec.expect.wrapping_add(1);
    }
    let ok = t.verify(7).err() == Some(VecError::Checksum);
    s.add("50078-J1-篡改必抓", ok, if ok { "" } else { "篡改未被发现" });

    // J1-8 重复用例号拒绝。
    let mut d = VectorLedger::new();
    let c0 = CaseSpec { id: 1200, kind: CaseKind::Media, a: 1, b: 1, expect: EXPECT_MIN + 2 };
    let _ = d.append(c0);
    let ok = d.append(c0).err() == Some(VecError::Duplicate) && d.len() == 1;
    s.add("50078-J1-重复拒绝", ok, if ok { "" } else { "重复用例入账" });

    // J1-9 越界参数拒绝（尺寸码 9 / 模式码 4 / 期望码 0）。
    let r1 = make_case(0, 1300, 9, 0, EXPECT_MIN);
    let r2 = make_case(0, 1301, 0, 4, EXPECT_MIN);
    let r3 = make_case(0, 1302, 0, 0, 0);
    let ok = r1.err() == Some(VecError::Param)
        && r2.err() == Some(VecError::Param)
        && r3.err() == Some(VecError::Expect);
    s.add("50078-J1-参数必拒", ok, if ok { "" } else { "越界参数被接受" });

    // J1-10 折叠对顺序敏感：同两条反序落账折叠不同；同序必然相同。
    let c1 = CaseSpec { id: 1400, kind: CaseKind::Size, a: 2, b: 0, expect: EXPECT_MIN + 4 };
    let c2 = CaseSpec { id: 1401, kind: CaseKind::Size, a: 2, b: 1, expect: EXPECT_MIN + 5 };
    let mut f1 = VectorLedger::new();
    let _ = f1.append(c1);
    let _ = f1.append(c2);
    let mut f2 = VectorLedger::new();
    let _ = f2.append(c2);
    let _ = f2.append(c1);
    let mut f3 = VectorLedger::new();
    let _ = f3.append(c1);
    let _ = f3.append(c2);
    let ok = f1.fold() == f3.fold() && f1.fold() != f2.fold();
    s.add("50078-J1-折叠次序", ok, if ok { "" } else { "折叠未区分次序" });

    // J1-11 空账折叠等于 FNV 偏移基（空集也有确定指纹）。
    let empty = VectorLedger::new();
    let ok = empty.is_empty() && empty.len() == 0 && empty.fold() == FNV_OFFSET;
    let det = format!("fold={:08x}", empty.fold());
    s.add_dyn("50078-J1-空账指纹", ok, &det);

    // J1-12 单条校验和与落账返回值一致，且同用例跨账本稳定。
    let mut x = VectorLedger::new();
    let mut y = VectorLedger::new();
    let shared = CaseSpec { id: 1500, kind: CaseKind::Bin, a: 3, b: 2, expect: EXPECT_MIN + 0x30 + 3 };
    let cx = x.append(shared);
    let cy = y.append(shared);
    let ok = cx == Ok(case_checksum(&shared)) && cy == cx;
    s.add("50078-J1-跨账稳定", ok, if ok { "" } else { "同用例校验和不稳定" });

    // 注入核：10 类坏样本全抓。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50078-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50078_checks_green() {
        let set = run_unx50078_checks();
        assert!(set.all_passed(), "UNX-F50078 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50078_generator_is_108() {
        assert_eq!(generate_paper_cases().len(), EXPECTED_CASES);
    }

    #[test]
    fn unx50078_checksum_detects_flip() {
        let c = CaseSpec { id: 1000, kind: CaseKind::Duplex, a: 0, b: 0, expect: EXPECT_MIN };
        let mut d = c;
        d.b = 2;
        assert_ne!(case_checksum(&c), case_checksum(&d));
    }
}
