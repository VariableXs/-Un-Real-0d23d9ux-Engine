//! UNX-F50012 色彩向量账（色块用例集+校验和）（0x42da0 段）
//!
//! 锚点：UNX-F50012-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 数据段 `SEG_BASE = 0x42da0`；诊断码闭集 = `SEG_BASE + n`（n 见 `LedgerError`）：
//!   +1 Empty      空向量 / 空账本 / 空用例集
//!   +2 CapFull    用例数超 `MAX_CASES` 或账本容量已满
//!   +3 Duplicate  向量名重复 / 用例 id 重复
//!   +4 Checksum   FNV-1a 校验和与登记值不符（搬运损坏）
//!   +5 Order      用例 id 非严格递增 / 账本登记序混乱
//!   +6 IdZero     用例 id 为 0（0 保留给「未登记」）
//!   +7 Weight     单条权重为 0 或权重总和超预算
//!   +8 Replay     重放偏差超 ΔE 预算，或出现非有限浮点
//!   +9 Name       向量名为空或超长
//!
//! 为什么「校验和 + 重放」双闸：用例集是跨管线搬运的资产。只存 id 与期望
//! Lab 无法证明搬动过程没被截断/篡改——校验和覆盖全部字段（含 Lab 的定点
//! 量化），负责抓搬运损坏；重放把期望值与现算值按 ΔE2000 比对，负责抓
//! 「谱系漂移」（实现改了口径、数据没改）。两闸职责不同，缺一不可。
//!
//! 结构清单：
//! - 数据类型：`ColorCase`（单条色块用例）、`ColorVector`（带校验和的用例集）、
//!   `VectorLedger`（定容注册表，登记名唯一）、`ReplayReport` / `LedgerReplay`
//! - 校验器：`Fnv64`（FNV-1a 64 位，跨平台逐位一致）、`checksum_of`
//! - 重放器：`replay_vector`（单向量）/ `VectorLedger::replay_all`（整账）
//! - 错误面：`LedgerError::code()` 全部落在 0x42da0 段闭集内
//! - 注入核：`injection_core` 七类坏样本（空/重复 id/乱序/篡改校验和/
//!   篡改期望 Lab/id=0/权重 0），逐类断言被显式拒绝

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{delta_e2000, rgb8_to_lab, Lab, Rgb8};

/// 诊断段基址（0x42da0 独占 0x20）。
pub const SEG_BASE: u32 = 0x42da0;

pub const E_EMPTY: u16 = 1;
pub const E_CAPFULL: u16 = 2;
pub const E_DUPLICATE: u16 = 3;
pub const E_CHECKSUM: u16 = 4;
pub const E_ORDER: u16 = 5;
pub const E_IDZERO: u16 = 6;
pub const E_WEIGHT: u16 = 7;
pub const E_REPLAY: u16 = 8;
pub const E_NAME: u16 = 9;

/// 单向量用例数上限。64 条够覆盖 6×6×6 网格的一个代表子集，又保证
/// 校验和计算与重放都是毫秒级、无堆增长压力。
pub const MAX_CASES: usize = 64;

/// 向量名最大字节数（ASCII 命名约定；中文名按 UTF-8 字节计）。
pub const MAX_VECTOR_NAME: usize = 32;

/// 单向量权重预算（w ∈ 1..=255，总和 ≤ 4096 ⇒ 覆盖 16 条满权重用例）。
pub const MAX_TOTAL_WEIGHT: u32 = 4096;

/// 重放容差（ΔE2000）。同一条浮点管线重算应当逐位相同，1e-3 只是给
/// 「外部登记的期望值」留出四舍五入余量，远低于 JND=1.0。
pub const REPLAY_TOL_E2000: f32 = 1e-3;

/// 向量账错误。`code()` 返回 `SEG_BASE + n`，落段内闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerError {
    Empty,
    CapFull,
    Duplicate,
    Checksum,
    Order,
    IdZero,
    Weight,
    Replay,
    Name,
}

impl LedgerError {
    /// 段内诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            LedgerError::Empty => E_EMPTY,
            LedgerError::CapFull => E_CAPFULL,
            LedgerError::Duplicate => E_DUPLICATE,
            LedgerError::Checksum => E_CHECKSUM,
            LedgerError::Order => E_ORDER,
            LedgerError::IdZero => E_IDZERO,
            LedgerError::Weight => E_WEIGHT,
            LedgerError::Replay => E_REPLAY,
            LedgerError::Name => E_NAME,
        };
        SEG_BASE + n as u32
    }

    /// 一行诊断文本（内核日志直接引用静态串，不分配）。
    pub fn message(self) -> &'static str {
        match self {
            LedgerError::Empty => "color vector empty",
            LedgerError::CapFull => "color vector capacity full",
            LedgerError::Duplicate => "duplicate vector name or case id",
            LedgerError::Checksum => "vector checksum mismatch",
            LedgerError::Order => "case ids not strictly increasing",
            LedgerError::IdZero => "case id zero reserved",
            LedgerError::Weight => "case weight out of budget",
            LedgerError::Replay => "replay drift exceeds budget",
            LedgerError::Name => "vector name empty or too long",
        }
    }
}

/// 非有限值判定。`f32` 的 NaN/±inf 会让校验和与 ΔE 都失去意义，
/// 所以在构造期就拦掉，而不是让下游拿 NaN 做比较。
fn finite(v: f32) -> bool {
    v == v && v > -1.0e30 && v < 1.0e30
}

/// FNV-1a 64 位累加器。选它而不是「求和」：乘法混淆保证「换序」与
/// 「单字节翻转」都能改变结果，且实现只有异或/乘法两条指令，跨平台逐位
/// 一致（无浮点参与字节流）。
#[derive(Clone, Copy)]
pub struct Fnv64 {
    h: u64,
}

impl Fnv64 {
    pub const fn new() -> Fnv64 {
        Fnv64 {
            h: 0xcbf29ce484222325,
        }
    }

    pub fn byte(&mut self, b: u8) {
        self.h ^= b as u64;
        self.h = self.h.wrapping_mul(0x100000001b3);
    }

    pub fn u16le(&mut self, v: u16) {
        self.byte(v as u8);
        self.byte((v >> 8) as u8);
    }

    pub fn u32le(&mut self, v: u32) {
        self.u16le(v as u16);
        self.u16le((v >> 16) as u16);
    }

    pub fn u64le(&mut self, v: u64) {
        self.u32le(v as u32);
        self.u32le((v >> 32) as u32);
    }

    /// 浮点按 1e-4 定点量化后入账。饱和转换（`as i32`）让 NaN→0、
    /// ±inf→边界值，因此校验和定义域被收窄为「有限且可量化」。
    pub fn f32q(&mut self, v: f32) {
        let scaled = v * 10000.0;
        let c = if scaled > 2.0e9 {
            2.0e9
        } else if scaled < -2.0e9 {
            -2.0e9
        } else {
            scaled
        };
        self.u32le(c as i32 as u32);
    }

    pub fn digest(self) -> u64 {
        self.h
    }
}

/// 单条色块用例：id + 8 位 sRGB 样本 + 期望 Lab（登记基线）+ 权重。
///
/// 字段公开是**自检注入核的需要**（要构造「期望 Lab 被篡改」「校验和被
/// 改写」的坏样本）；正常路径只经 `new` / `registered` 构造，两者都做
/// id/权重/有限性校验。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorCase {
    pub id: u16,
    pub rgb: Rgb8,
    pub expect_lab: Lab,
    pub weight: u8,
}

impl ColorCase {
    /// 登记一条用例，期望 Lab 由当前色彩内核即时推导（自洽基线）。
    pub fn new(id: u16, rgb: Rgb8, weight: u8) -> Result<ColorCase, LedgerError> {
        ColorCase::registered(id, rgb, rgb8_to_lab(rgb), weight)
    }

    /// 登记外部管线算出的期望 Lab（跨管线基线）；校验规则与 `new` 相同。
    pub fn registered(
        id: u16,
        rgb: Rgb8,
        expect_lab: Lab,
        weight: u8,
    ) -> Result<ColorCase, LedgerError> {
        if id == 0 {
            return Err(LedgerError::IdZero);
        }
        if weight == 0 {
            return Err(LedgerError::Weight);
        }
        if !finite(expect_lab.l) || !finite(expect_lab.a) || !finite(expect_lab.b) {
            return Err(LedgerError::Replay);
        }
        Ok(ColorCase {
            id,
            rgb,
            expect_lab,
            weight,
        })
    }

    /// 单条用例偏差：期望 Lab 与现算 Lab 的 ΔE2000。
    pub fn drift(&self) -> f32 {
        delta_e2000(self.expect_lab, rgb8_to_lab(self.rgb))
    }

    /// 把全部字段喂进校验和（顺序固定：id→rgb→weight→Lab）。
    fn feed(&self, h: &mut Fnv64) {
        h.u16le(self.id);
        h.byte(self.rgb.r);
        h.byte(self.rgb.g);
        h.byte(self.rgb.b);
        h.byte(self.weight);
        h.f32q(self.expect_lab.l);
        h.f32q(self.expect_lab.a);
        h.f32q(self.expect_lab.b);
    }
}

/// 校验和：用例数打头，随后逐条按固定字段序入账。长度也参与，
/// 因此「截尾」与「追加」都会改变结果。
pub fn checksum_of(cases: &[ColorCase]) -> u64 {
    let mut h = Fnv64::new();
    h.u16le(cases.len() as u16);
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(c) = cases.get(i) {
            c.feed(&mut h);
        }
        i += 1;
    }
    h.digest()
}

/// 色彩向量：命名的用例集 + 登记校验和。
#[derive(Clone, Debug, PartialEq)]
pub struct ColorVector {
    pub name: String,
    pub cases: Vec<ColorCase>,
    pub checksum: u64,
}

impl ColorVector {
    /// 构造向量账。空集/超容/名字非法/重复 id/乱序/权重越界一律显式拒绝。
    ///
    /// 判序顺序有意为之：先查重复、再查递增——`[1,1]` 报 Duplicate 而不是
    /// Order，因为「两行同 id」的根因是复制粘贴，而「倒序」的根因是排序
    /// 逻辑错，诊断文本必须指向真正的根因。
    pub fn build(name: &str, cases: Vec<ColorCase>) -> Result<ColorVector, LedgerError> {
        if cases.is_empty() {
            return Err(LedgerError::Empty);
        }
        if cases.len() > MAX_CASES {
            return Err(LedgerError::CapFull);
        }
        if name.is_empty() || name.len() > MAX_VECTOR_NAME {
            return Err(LedgerError::Name);
        }
        let mut weight = 0u32;
        let mut i = 0usize;
        while i < cases.len() {
            let a = match cases.get(i) {
                Some(c) => c,
                None => return Err(LedgerError::Empty),
            };
            weight = weight.saturating_add(a.weight as u32);
            let mut j = i + 1;
            while j < cases.len() {
                if let Some(b) = cases.get(j) {
                    if b.id == a.id {
                        return Err(LedgerError::Duplicate);
                    }
                }
                j += 1;
            }
            if i > 0 {
                if let Some(p) = cases.get(i - 1) {
                    if p.id >= a.id {
                        return Err(LedgerError::Order);
                    }
                }
            }
            i += 1;
        }
        if weight > MAX_TOTAL_WEIGHT {
            return Err(LedgerError::Weight);
        }
        let checksum = checksum_of(&cases);
        Ok(ColorVector {
            name: name.to_string(),
            cases,
            checksum,
        })
    }

    /// 按当前字段重算校验和（不改登记值）。
    pub fn checksum_now(&self) -> u64 {
        checksum_of(&self.cases)
    }

    /// 只验校验和：搬运/存储层的完整性闸。
    pub fn verify(&self) -> Result<(), LedgerError> {
        if self.checksum == self.checksum_now() {
            Ok(())
        } else {
            Err(LedgerError::Checksum)
        }
    }

    pub fn len(&self) -> usize {
        self.cases.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }
}

/// 单向量重放报告。`digest` 复述登记校验和，方便调用方做逐位比对。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayReport {
    pub cases: usize,
    pub max_delta: f32,
    pub digest: u64,
}

/// 整账重放报告：向量数、用例总数、全账最大偏差、按登记序折叠的摘要。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LedgerReplay {
    pub vectors: usize,
    pub cases: usize,
    pub max_delta: f32,
    pub digest: u64,
}

/// 重放一个向量：校验和 → id 闭集/唯一/递增 → 逐条 ΔE 偏差。
///
/// 返回的 `max_delta` 是全部用例偏差的最大值，用于「用例可复跑」判据的
/// 可审计明细；一旦超预算即整向量失败，不做「部分通过」的模糊结论。
pub fn replay_vector(v: &ColorVector) -> Result<ReplayReport, LedgerError> {
    if v.cases.is_empty() {
        return Err(LedgerError::Empty);
    }
    if v.checksum != v.checksum_now() {
        return Err(LedgerError::Checksum);
    }
    let mut max_delta = 0.0f32;
    let mut i = 0usize;
    while i < v.cases.len() {
        let a = match v.cases.get(i) {
            Some(c) => c,
            None => return Err(LedgerError::Empty),
        };
        if a.id == 0 {
            return Err(LedgerError::IdZero);
        }
        let mut j = i + 1;
        while j < v.cases.len() {
            if let Some(b) = v.cases.get(j) {
                if b.id == a.id {
                    return Err(LedgerError::Duplicate);
                }
            }
            j += 1;
        }
        if i > 0 {
            if let Some(p) = v.cases.get(i - 1) {
                if p.id >= a.id {
                    return Err(LedgerError::Order);
                }
            }
        }
        let d = a.drift();
        if !finite(d) || fabsf(d) > REPLAY_TOL_E2000 {
            return Err(LedgerError::Replay);
        }
        if d > max_delta {
            max_delta = d;
        }
        i += 1;
    }
    Ok(ReplayReport {
        cases: v.cases.len(),
        max_delta,
        digest: v.checksum,
    })
}

/// 向量账本：定容注册表，名字唯一，登记前必须先通过重放。
#[derive(Clone, Debug)]
pub struct VectorLedger {
    cap: usize,
    vectors: Vec<ColorVector>,
    cases: usize,
}

impl VectorLedger {
    /// 新建账本。`cap == 0` 时任何登记都报 CapFull（显式，不是静默 no-op）。
    pub fn new(cap: usize) -> VectorLedger {
        VectorLedger {
            cap,
            vectors: Vec::new(),
            cases: 0,
        }
    }

    /// 登记一个向量，返回其在账内的下标。
    ///
    /// 顺序：容量 → 重放（坏账不入账）→ 重名。重放放在重名之前，是为了
    /// 让「同名且损坏」的报告优先指向损坏——名字冲突可以改名绕过，损坏
    /// 不能。
    pub fn register(&mut self, v: ColorVector) -> Result<usize, LedgerError> {
        if self.vectors.len() >= self.cap {
            return Err(LedgerError::CapFull);
        }
        replay_vector(&v)?;
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(x) = self.vectors.get(i) {
                if x.name == v.name {
                    return Err(LedgerError::Duplicate);
                }
            }
            i += 1;
        }
        self.cases = self.cases.saturating_add(v.cases.len());
        self.vectors.push(v);
        Ok(self.vectors.len() - 1)
    }

    /// 按名检索（名唯一，故命中至多一条）。
    pub fn find(&self, name: &str) -> Option<&ColorVector> {
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                if v.name == name {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    /// 整账重放：逐向量重放，按登记序折叠摘要（顺序敏感，防重排）。
    pub fn replay_all(&self) -> Result<LedgerReplay, LedgerError> {
        if self.vectors.is_empty() {
            return Err(LedgerError::Empty);
        }
        let mut h = Fnv64::new();
        h.u16le(self.vectors.len() as u16);
        let mut cases = 0usize;
        let mut max_delta = 0.0f32;
        let mut i = 0usize;
        while i < self.vectors.len() {
            let v = match self.vectors.get(i) {
                Some(v) => v,
                None => return Err(LedgerError::Empty),
            };
            let r = replay_vector(v)?;
            cases = cases.saturating_add(r.cases);
            if r.max_delta > max_delta {
                max_delta = r.max_delta;
            }
            h.u64le(r.digest);
            i += 1;
        }
        Ok(LedgerReplay {
            vectors: self.vectors.len(),
            cases,
            max_delta,
            digest: h.digest(),
        })
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// 账内用例总数（登记时累加，不走重放，供容量审计）。
    pub fn total_cases(&self) -> usize {
        self.cases
    }

    /// 账内权重总和。权重是「本用例在验收里的分量」，总和必须守恒。
    pub fn total_weight(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                let mut j = 0usize;
                while j < v.cases.len() {
                    if let Some(c) = v.cases.get(j) {
                        sum = sum.saturating_add(c.weight as u32);
                    }
                    j += 1;
                }
            }
            i += 1;
        }
        sum
    }
}

// ---------------------------------------------------------------------------
// 注入核：七类坏样本必须全部被抓
// ---------------------------------------------------------------------------

fn case(id: u16, r: u8, g: u8, b: u8, w: u8) -> ColorCase {
    match ColorCase::new(id, Rgb8::new(r, g, b), w) {
        Ok(c) => c,
        // 注入核只喂合法形参（id≠0、w≠0 由调用点保证）；此分支不改变
        // 判定语义，仅让「构造失败」也不会 panic。
        Err(_) => ColorCase {
            id: 1,
            rgb: Rgb8::BLACK,
            expect_lab: Lab::new(0.0, 0.0, 0.0),
            weight: 1,
        },
    }
}

/// 七类坏样本注入：返回（被抓数, 总数）。
///
/// 每类都断言**具体的**诊断码，而不是「只要 Err 就算过」——错误码错位
/// 意味着下游告警会指向错误的根因。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1 空用例集
    if ColorVector::build("empty", Vec::new()).err() == Some(LedgerError::Empty) {
        caught += 1;
    }
    // 2 重复 id（复制粘贴）
    if ColorVector::build("dup", vec![case(1, 10, 20, 30, 5), case(1, 40, 50, 60, 5)])
        .err()
        == Some(LedgerError::Duplicate)
    {
        caught += 1;
    }
    // 3 乱序（排序逻辑错）
    if ColorVector::build("order", vec![case(2, 10, 20, 30, 5), case(1, 40, 50, 60, 5)])
        .err()
        == Some(LedgerError::Order)
    {
        caught += 1;
    }
    // 4 校验和被改（搬运损坏）
    if let Ok(mut v) = ColorVector::build("sum", vec![case(1, 9, 9, 9, 1)]) {
        v.checksum ^= 0x1;
        if replay_vector(&v).err() == Some(LedgerError::Checksum) {
            caught += 1;
        }
    }
    // 5 期望 Lab 被篡改（谱系漂移）
    if let Ok(bad_lab) = ColorCase::registered(
        1,
        Rgb8::new(200, 30, 30),
        Lab::new(80.0, 60.0, 50.0),
        3,
    ) {
        if let Ok(v) = ColorVector::build("drift", vec![bad_lab]) {
            if replay_vector(&v).err() == Some(LedgerError::Replay) {
                caught += 1;
            }
        }
    }
    // 6 id 为 0（保留值）
    if ColorCase::new(0, Rgb8::WHITE, 1).err() == Some(LedgerError::IdZero) {
        caught += 1;
    }
    // 7 权重为 0（等于把用例从验收里静默剔除）
    if ColorCase::new(1, Rgb8::WHITE, 0).err() == Some(LedgerError::Weight) {
        caught += 1;
    }
    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 构造一条 8 用例的「标准账」：从近黑到近白的灰阶 + 三原色。
fn standard_vector(name: &str) -> Result<ColorVector, LedgerError> {
    let mut cases = Vec::with_capacity(8);
    cases.push(case(1, 0, 0, 0, 10));
    cases.push(case(2, 32, 32, 32, 10));
    cases.push(case(3, 64, 64, 64, 10));
    cases.push(case(4, 128, 128, 128, 20));
    cases.push(case(5, 192, 192, 192, 10));
    cases.push(case(6, 255, 0, 0, 20));
    cases.push(case(7, 0, 255, 0, 20));
    cases.push(case(8, 0, 0, 255, 20));
    ColorVector::build(name, cases)
}

pub fn run_unx50012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50012");

    // J1-1 校验和确定性：同内容两次构建逐位相同
    let a = standard_vector("std-a");
    let b = standard_vector("std-a");
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x.checksum == y.checksum && x.checksum != 0,
        _ => false,
    };
    s.add("50012-J1-校验和确定", ok, if ok { "" } else { "同内容校验和不一致" });

    // J1-2 校验和敏感性：单字段改动必改变结果
    let mut mutated_cases = Vec::new();
    mutated_cases.push(case(1, 0, 0, 0, 10));
    mutated_cases.push(case(2, 32, 32, 33, 10)); // g→b 差 1
    mutated_cases.push(case(3, 64, 64, 64, 10));
    mutated_cases.push(case(4, 128, 128, 128, 20));
    mutated_cases.push(case(5, 192, 192, 192, 10));
    mutated_cases.push(case(6, 255, 0, 0, 20));
    mutated_cases.push(case(7, 0, 255, 0, 20));
    mutated_cases.push(case(8, 0, 0, 255, 20));
    let m = ColorVector::build("std-a", mutated_cases);
    let ok = match (&a, &m) {
        (Ok(x), Ok(y)) => x.checksum != y.checksum,
        _ => false,
    };
    s.add("50012-J1-校验和敏感", ok, if ok { "" } else { "单字节改动未被检出" });

    // J1-3 空集必拒
    let ok = ColorVector::build("e", Vec::new()).err() == Some(LedgerError::Empty);
    s.add("50012-J1-空集必拒", ok, if ok { "" } else { "空集被接受" });

    // J1-4 重复 id 必拒（且报 Duplicate 而非 Order）
    let dup = vec![case(3, 1, 2, 3, 1), case(3, 4, 5, 6, 1)];
    let ok = ColorVector::build("d", dup).err() == Some(LedgerError::Duplicate);
    s.add("50012-J1-重复必拒", ok, if ok { "" } else { "重复 id 被接受" });

    // J1-5 乱序必拒
    let rev = vec![case(9, 1, 2, 3, 1), case(4, 4, 5, 6, 1)];
    let ok = ColorVector::build("r", rev).err() == Some(LedgerError::Order);
    s.add("50012-J1-乱序必拒", ok, if ok { "" } else { "倒序 id 被接受" });

    // J1-6 权重预算：单条 0 拒、总和超预算拒
    let zero_w = ColorCase::new(1, Rgb8::WHITE, 0).err() == Some(LedgerError::Weight);
    let mut heavy = Vec::with_capacity(17);
    let mut i = 0u16;
    while i < 17 {
        heavy.push(case(i + 1, 10, 10, 10, 255));
        i += 1;
    }
    let over = ColorVector::build("h", heavy).err() == Some(LedgerError::Weight);
    let ok = zero_w && over;
    let det = format!("w0={} over={}", zero_w, over);
    s.add_dyn("50012-J1-权重预算", ok, &det);

    // J1-7 名字非法（空 / 超长）必拒
    let ok = ColorVector::build("", vec![case(1, 1, 1, 1, 1)]).err() == Some(LedgerError::Name);
    let long = "0123456789012345678901234567890123"; // 34 字节 > 32
    let ok2 = ColorVector::build(long, vec![case(1, 1, 1, 1, 1)]).err() == Some(LedgerError::Name);
    let ok = ok && ok2;
    s.add("50012-J1-名字必拒", ok, if ok { "" } else { "非法名被接受" });

    // J1-8 账本：重名拒、容量满拒、find 命中与未命中
    let mut led = VectorLedger::new(2);
    let r1 = led.register(match standard_vector("v1") {
        Ok(v) => v,
        Err(_) => return s,
    });
    let r2 = led.register(match standard_vector("v1") {
        Ok(v) => v,
        Err(_) => return s,
    });
    let r3 = led.register(match standard_vector("v2") {
        Ok(v) => v,
        Err(_) => return s,
    });
    let r4 = led.register(match standard_vector("v3") {
        Ok(v) => v,
        Err(_) => return s,
    });
    let ok = r1 == Ok(0)
        && r2.err() == Some(LedgerError::Duplicate)
        && r3 == Ok(1)
        && r4.err() == Some(LedgerError::CapFull);
    let det = format!("r1={:?} r4={:?}", r1, r4);
    s.add_dyn("50012-J1-账本闸", ok, &det);

    // J1-9 find 命中/未命中
    let ok = led.find("v2").is_some() && led.find("nope").is_none() && led.len() == 2;
    s.add("50012-J1-检索", ok, if ok { "" } else { "find 结果错" });

    // J1-10 J1 核心：用例可复跑（两次重放逐位一致，且零偏差）
    let ok = match (&a, &a) {
        (Ok(x), Ok(_)) => match (replay_vector(x), replay_vector(x)) {
            (Ok(r1), Ok(r2)) => r1 == r2 && r1.cases == 8 && fabsf(r1.max_delta) < 1e-6,
            _ => false,
        },
        _ => false,
    };
    let det = match &a {
        Ok(x) => match replay_vector(x) {
            Ok(r) => format!("cases={} maxd={:.4}", r.cases, r.max_delta),
            Err(e) => format!("err={:?}", e),
        },
        Err(e) => format!("build={:?}", e),
    };
    s.add_dyn("50012-J1-用例可复跑", ok, &det);

    // J1-11 篡改期望 Lab 必拒（谱系漂移检测）
    let ok = match ColorCase::registered(1, Rgb8::new(200, 30, 30), Lab::new(80.0, 60.0, 50.0), 1) {
        Ok(c) => match ColorVector::build("t", vec![c]) {
            Ok(v) => replay_vector(&v).err() == Some(LedgerError::Replay),
            Err(_) => false,
        },
        Err(_) => false,
    };
    s.add("50012-J1-漂移必拒", ok, if ok { "" } else { "漂移未被检出" });

    // J1-12 篡改校验和必拒，且 verify() 与 replay 结论一致
    let ok = match standard_vector("c") {
        Ok(mut v) => {
            v.checksum = v.checksum.wrapping_add(7);
            v.verify().err() == Some(LedgerError::Checksum)
                && replay_vector(&v).err() == Some(LedgerError::Checksum)
        }
        Err(_) => false,
    };
    s.add("50012-J1-篡改必拒", ok, if ok { "" } else { "校验和篡改未被检出" });

    // J1-13 权重守恒：账本 total_weight == 逐条求和
    let mut sum = 0u32;
    let mut i = 0usize;
    while i < led.len() {
        if let Some(v) = led.find(if i == 0 { "v1" } else { "v2" }) {
            let mut j = 0usize;
            while j < v.cases.len() {
                if let Some(c) = v.cases.get(j) {
                    sum = sum.saturating_add(c.weight as u32);
                }
                j += 1;
            }
        }
        i += 1;
    }
    let ok = led.total_weight() == sum && sum == 240;
    let det = format!("sum={} total={}", sum, led.total_weight());
    s.add_dyn("50012-J1-权重守恒", ok, &det);

    // J1-14 整账重放：向量数/用例数/摘要确定
    let ok = match (led.replay_all(), led.replay_all()) {
        (Ok(x), Ok(y)) => x == y && x.vectors == 2 && x.cases == 16 && x.digest != 0,
        _ => false,
    };
    s.add("50012-J1-整账重放", ok, if ok { "" } else { "整账重放不稳定" });

    // J1-15 七类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50012-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50012_checks_green() {
        let set = run_unx50012_checks();
        assert!(set.all_passed(), "UNX-F50012 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50012_checksum_is_order_sensitive() {
        let a = checksum_of(&[case(1, 1, 2, 3, 1), case(2, 4, 5, 6, 1)]);
        let b = checksum_of(&[case(2, 1, 2, 3, 1), case(1, 4, 5, 6, 1)]);
        assert!(a != b, "换序必须改变校验和");
    }

    #[test]
    fn unx50012_replay_zero_drift() {
        let v = match standard_vector("t") {
            Ok(v) => v,
            Err(_) => return,
        };
        match replay_vector(&v) {
            Ok(r) => assert_eq!(r.cases, 8),
            Err(_) => assert!(false, "standard vector must replay"),
        }
    }
}
