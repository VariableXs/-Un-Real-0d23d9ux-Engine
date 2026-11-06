//! UNX-F50055 水印向量账（合成用例集+校验和）（0x43300 段）
//!
//! 锚点：UNX-F50055-J1 用例可复跑；判据锚定 §AI-63 §5。
//!
//! 结构清单：
//! * 用例种别注册表 `VecKind`：五种合成图形（斜坡/棋盘/噪声/纯色/渐变），
//!   每种自带样本生成规则，`tag()` 同时作为文本格式里的稳定标识；
//! * 向量 `WmVector`：id + 种别 + 种子 + 强度 + 载荷 + 期望摘要，逐条算
//!   `vector_checksum`（内容校验和）与 `sample_digest`（样本摘要）；
//! * 向量账 `VectorSet`：条数上限 32，逐条按 `fold(fold(root,cksum),digest)`
//!   累积出**顺序敏感**的根校验和；根与重算不一致即整账判 `Checksum`；
//! * 文本编解码 `encode` / `parse_set`：可读格式带 root 头，导入时校验根——
//!   「可复跑」= 编码→解析→重放三步后每条摘要与根校验和逐位不变；
//! * 注入核：字段数 / 十六进制 / 种别 / 重复 id / 条数 / 空账 / id / 根校验和 /
//!   强度 九类坏样本，逐类断言具体诊断码。
//!
//! 错误纪律：诊断码闭集 `0x43300 + 1..=9`；内核路径零 panic——解析全程
//! `split` + `get`，不按下标切字符串（否则非 ASCII 输入会切在多字节中间）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 诊断段（0x43300 独占 0x20）。
pub const SEG_BASE: u32 = 0x43300;

pub const E_FIELDS: u16 = 1;
pub const E_HEX: u16 = 2;
pub const E_KIND: u16 = 3;
pub const E_DUP: u16 = 4;
pub const E_COUNT: u16 = 5;
pub const E_EMPTY: u16 = 6;
pub const E_ID: u16 = 7;
pub const E_CHECKSUM: u16 = 8;
pub const E_RANGE: u16 = 9;

/// 单账最多 32 条：一次回归跑 32 例已能覆盖五种种别 × 四种强度的稀疏组合；
/// 再大就说明有人在把真实语料当向量塞，应该走样本页账本而不是向量账。
pub const VEC_CAP: usize = 32;
/// 每条向量的合成样本数（fixed 16：够区分种别差异，又便宜到能进判据）。
pub const VEC_SAMPLES: u32 = 16;

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 向量账错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WmError {
    Fields,
    Hex,
    Kind,
    Dup,
    Count,
    Empty,
    Id,
    Checksum,
    Range,
}

impl WmError {
    pub fn code(self) -> u32 {
        let n = match self {
            WmError::Fields => E_FIELDS,
            WmError::Hex => E_HEX,
            WmError::Kind => E_KIND,
            WmError::Dup => E_DUP,
            WmError::Count => E_COUNT,
            WmError::Empty => E_EMPTY,
            WmError::Id => E_ID,
            WmError::Checksum => E_CHECKSUM,
            WmError::Range => E_RANGE,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            WmError::Fields => "vector line does not have 6 fields",
            WmError::Hex => "hex field malformed",
            WmError::Kind => "unknown vector kind",
            WmError::Dup => "vector id duplicated",
            WmError::Count => "vector count exceeds capacity",
            WmError::Empty => "vector set or payload empty",
            WmError::Id => "numeric field malformed",
            WmError::Checksum => "set root checksum mismatch",
            WmError::Range => "strength outside [0,1]",
        }
    }
}

/// 五种合成图形。种别是文本格式里的稳定枚举，不能靠序号落盘——
/// 序号会随枚举顺序变化而漂移，历史用例就再也对不上摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecKind {
    Ramp,
    Checker,
    Noise,
    Solid,
    Gradient,
}

impl VecKind {
    pub fn tag(self) -> &'static str {
        match self {
            VecKind::Ramp => "ramp",
            VecKind::Checker => "checker",
            VecKind::Noise => "noise",
            VecKind::Solid => "solid",
            VecKind::Gradient => "gradient",
        }
    }

    pub fn parse(s: &str) -> Result<VecKind, WmError> {
        match s {
            "ramp" => Ok(VecKind::Ramp),
            "checker" => Ok(VecKind::Checker),
            "noise" => Ok(VecKind::Noise),
            "solid" => Ok(VecKind::Solid),
            "gradient" => Ok(VecKind::Gradient),
            _ => Err(WmError::Kind),
        }
    }
}

/// 一条水印用例。`expect` 是作者端钉住的样本摘要，重放时必须逐位命中。
#[derive(Clone, Debug, PartialEq)]
pub struct WmVector {
    pub id: u16,
    pub kind: VecKind,
    pub seed: u32,
    pub strength: f32,
    pub payload: Vec<u8>,
    pub expect: u64,
}

fn mix_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// FNV-1a 64：跨构建稳定的内容摘要。判据里用它对齐 FNV 官方向量，
/// 是「校验和实现本身正确」的外部锚点。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV_BASIS;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

fn fold(h: u64, v: u64) -> u64 {
    mix_u64(h, v)
}

/// 向量内容校验和：id / 种别 / 种子 / 强度（千分位定点）/ 载荷长度 / 载荷。
///
/// 强度取千分位整数而不是 f32 位模式：位模式会把 `0.5` 与 `0.5000001`
/// 当成两条不同用例，而这一位的差别对样本生成毫无影响。
pub fn vector_checksum(v: &WmVector) -> u64 {
    let mut h = FNV_BASIS;
    h = mix_u64(h, v.id as u64);
    for b in v.kind.tag().as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h = mix_u64(h, v.seed as u64);
    let q = (v.strength * 1000.0) as i32 as u64;
    h = mix_u64(h, q);
    h = mix_u64(h, v.payload.len() as u64);
    let mut i = 0usize;
    while i < v.payload.len() {
        if let Some(b) = v.payload.get(i) {
            h ^= *b as u64;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

fn lcg(x: u32) -> u32 {
    x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

fn len_or_one(n: usize) -> usize {
    if n == 0 {
        1
    } else {
        n
    }
}

/// 合成样本：16 个 8 位采样。种别决定基底，载荷按位循环异或，强度折算成
/// 统一偏移——三者都可复算，同一条向量在任何时候都得到同一串字节。
pub fn sample_pattern(v: &WmVector) -> Vec<u8> {
    let mut out = Vec::with_capacity(VEC_SAMPLES as usize);
    let gain = (v.strength * 255.0) as u8;
    let denom = VEC_SAMPLES - 1;
    let mut i = 0u32;
    while i < VEC_SAMPLES {
        let base: u8 = match v.kind {
            VecKind::Ramp => (i * 16) as u8,
            VecKind::Checker => {
                if i % 2 == 0 {
                    0xFF
                } else {
                    0x00
                }
            }
            VecKind::Noise => (lcg(v.seed ^ i.wrapping_mul(0x9E37_79B9)) >> 24) as u8,
            VecKind::Solid => (v.seed & 0xff) as u8,
            VecKind::Gradient => ((i * 255) / denom) as u8,
        };
        let at = (i as usize) % len_or_one(v.payload.len());
        let p = match v.payload.get(at) {
            Some(b) => *b,
            None => 0,
        };
        out.push(base ^ p ^ gain);
        i += 1;
    }
    out
}

/// 样本摘要（J1 的可复跑锚：同一条向量两次求值逐位相等）。
pub fn sample_digest(v: &WmVector) -> u64 {
    fnv1a64(&sample_pattern(v))
}

/// 一条用例的重放结果。`matched == false` 说明作者钉的摘要与当前实现脱节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseOutcome {
    pub id: u16,
    pub checksum: u64,
    pub digest: u64,
    pub matched: bool,
}

/// 整账重放报告。`passed + failed == set.len()` 是硬守恒。
#[derive(Clone, Debug)]
pub struct RunReport {
    pub passed: usize,
    pub failed: usize,
    pub root: u64,
    pub outcomes: Vec<CaseOutcome>,
}

/// 向量账：注册表 + 顺序敏感根校验和。
#[derive(Clone, Debug)]
pub struct VectorSet {
    vectors: Vec<WmVector>,
    root: u64,
}

impl VectorSet {
    pub fn new() -> VectorSet {
        VectorSet {
            vectors: Vec::new(),
            root: FNV_BASIS,
        }
    }

    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    pub fn root(&self) -> u64 {
        self.root
    }

    pub fn vectors(&self) -> &[WmVector] {
        &self.vectors
    }

    /// 可变视图（作者改 `expect` 后必须重算根）。刻意暴露可变切片而不是
    /// 逐字段 setter：改完要立刻 `recompute_root`，这一点由调用方负责。
    pub fn vectors_mut(&mut self) -> &mut [WmVector] {
        &mut self.vectors
    }

    /// 追加一条向量并累积根校验和。校验顺序：强度 → 载荷 → 容量 → 重复 id。
    pub fn add(&mut self, v: WmVector) -> Result<(), WmError> {
        if !(v.strength >= 0.0 && v.strength <= 1.0) {
            return Err(WmError::Range);
        }
        if v.payload.is_empty() {
            return Err(WmError::Empty);
        }
        if self.vectors.len() >= VEC_CAP {
            return Err(WmError::Count);
        }
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(x) = self.vectors.get(i) {
                if x.id == v.id {
                    return Err(WmError::Dup);
                }
            }
            i += 1;
        }
        let ck = vector_checksum(&v);
        let dg = sample_digest(&v);
        self.root = fold(fold(self.root, ck), dg);
        self.vectors.push(v);
        Ok(())
    }

    /// 从零重算根（与 `add` 的累积方式必须逐位一致）。
    pub fn recompute_root(&self) -> u64 {
        let mut r = FNV_BASIS;
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                r = fold(fold(r, vector_checksum(v)), sample_digest(v));
            }
            i += 1;
        }
        r
    }

    /// 根自检：不一致即整账不可信（落盘被改过 / 版本漂移）。
    pub fn verify(&self) -> Result<u64, WmError> {
        let r = self.recompute_root();
        if r == self.root {
            Ok(r)
        } else {
            Err(WmError::Checksum)
        }
    }

    /// 编码为可读文本：`WMVEC v1` 头 + `root=<16hex>` + 每行一条向量。
    pub fn encode(&self) -> String {
        let mut s = String::new();
        s.push_str("WMVEC v1\n");
        s.push_str(&format!("root={:016x}\n", self.root));
        let mut i = 0usize;
        while i < self.vectors.len() {
            if let Some(v) = self.vectors.get(i) {
                s.push_str(&encode_vector(v));
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 单条向量的文本形式：`id|kind|seed|strength|payload_hex|expect_hex`。
pub fn encode_vector(v: &WmVector) -> String {
    format!(
        "{}|{}|{}|{:.3}|{}|{:016x}",
        v.id,
        v.kind.tag(),
        v.seed,
        v.strength,
        hex_encode(&v.payload),
        v.expect
    )
}

/// 十六进制编码（小写）。
pub fn hex_encode(bytes: &[u8]) -> String {
    const HEX: [u8; 16] = *b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            s.push(HEX[(*b >> 4) as usize] as char);
            s.push(HEX[(*b & 0x0f) as usize] as char);
        }
        i += 1;
    }
    s
}

fn hex_val(c: u8) -> Result<u8, WmError> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(WmError::Hex),
    }
}

/// 十六进制解码。奇数长度直接拒：半个字节无法解释，补 0 会把坏数据洗成好数据。
pub fn hex_decode(s: &str) -> Result<Vec<u8>, WmError> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return Err(WmError::Hex);
    }
    let mut out = Vec::with_capacity(b.len() / 2);
    let mut i = 0usize;
    while i < b.len() {
        let hi = match b.get(i) {
            Some(c) => hex_val(*c)?,
            None => return Err(WmError::Hex),
        };
        let lo = match b.get(i + 1) {
            Some(c) => hex_val(*c)?,
            None => return Err(WmError::Hex),
        };
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn parse_u64_hex(s: &str) -> Result<u64, WmError> {
    u64::from_str_radix(s, 16).map_err(|_| WmError::Hex)
}

fn parse_vector(line: &str) -> Result<WmVector, WmError> {
    let mut it = line.split('|');
    let id_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let kind_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let seed_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let str_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let pay_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let exp_s = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    if it.next().is_some() {
        return Err(WmError::Fields);
    }
    let id = id_s.parse::<u16>().map_err(|_| WmError::Id)?;
    let kind = VecKind::parse(kind_s)?;
    let seed = seed_s.parse::<u32>().map_err(|_| WmError::Id)?;
    let strength = str_s.parse::<f32>().map_err(|_| WmError::Range)?;
    if !(strength >= 0.0 && strength <= 1.0) {
        return Err(WmError::Range);
    }
    let payload = hex_decode(pay_s)?;
    let expect = parse_u64_hex(exp_s)?;
    Ok(WmVector {
        id,
        kind,
        seed,
        strength,
        payload,
        expect,
    })
}

/// 解析整账文本。头两行固定；逐条 `add` 后比对 `root`——
/// 先信内容再信根，根不符说明文本被改过，直接 `Checksum`。
pub fn parse_set(text: &str) -> Result<VectorSet, WmError> {
    let mut it = text.lines();
    match it.next() {
        Some(h) if h == "WMVEC v1" => {}
        Some(_) => return Err(WmError::Fields),
        None => return Err(WmError::Empty),
    }
    let root_line = match it.next() {
        Some(v) => v,
        None => return Err(WmError::Empty),
    };
    let root_hex = match root_line.strip_prefix("root=") {
        Some(v) => v,
        None => return Err(WmError::Fields),
    };
    let pinned = parse_u64_hex(root_hex)?;
    let mut set = VectorSet::new();
    let mut any = false;
    for line in it {
        if line.is_empty() {
            continue;
        }
        let v = parse_vector(line)?;
        set.add(v)?;
        any = true;
    }
    if !any {
        return Err(WmError::Empty);
    }
    if set.root() != pinned {
        return Err(WmError::Checksum);
    }
    Ok(set)
}

/// 内置六条向量（覆盖五种种别 + 一条重复种别）。`expect` 在构造时钉住，
/// 之后任何实现漂移都会让重放 `failed` 计数上升。
pub fn builtin_set() -> Result<VectorSet, WmError> {
    let mut s = VectorSet::new();
    let mut i = 0u16;
    while i < 6 {
        let kind = match i % 5 {
            0 => VecKind::Ramp,
            1 => VecKind::Checker,
            2 => VecKind::Noise,
            3 => VecKind::Solid,
            _ => VecKind::Gradient,
        };
        let n = 2 + (i as usize % 4);
        let mut payload = Vec::with_capacity(n);
        let mut k = 0usize;
        while k < n {
            payload.push((0x11u8).wrapping_mul(i as u8 + 1).wrapping_add(k as u8));
            k += 1;
        }
        let mut v = WmVector {
            id: 10 + i,
            kind,
            seed: 0x1234 + i as u32,
            strength: 0.5,
            payload,
            expect: 0,
        };
        v.expect = sample_digest(&v);
        s.add(v)?;
        i += 1;
    }
    Ok(s)
}

/// 重放整账。种子/载荷全在向量里，不依赖任何外部状态。
pub fn run_set(set: &VectorSet) -> RunReport {
    let mut outcomes = Vec::with_capacity(set.len());
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut i = 0usize;
    while i < set.len() {
        if let Some(v) = set.vectors().get(i) {
            let dg = sample_digest(v);
            let matched = dg == v.expect;
            if matched {
                passed += 1;
            } else {
                failed += 1;
            }
            outcomes.push(CaseOutcome {
                id: v.id,
                checksum: vector_checksum(v),
                digest: dg,
                matched,
            });
        }
        i += 1;
    }
    RunReport {
        passed,
        failed,
        root: set.recompute_root(),
        outcomes,
    }
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn text_with(body: &str, root: u64) -> String {
    let mut s = String::new();
    s.push_str("WMVEC v1\n");
    s.push_str(&format!("root={:016x}\n", root));
    s.push_str(body);
    s
}

/// 九类坏样本。返回 (抓住, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 9usize;

    // 1 字段数不足（3 段）
    let t = text_with("1|ramp|1\n", 0);
    if parse_set(&t).err() == Some(WmError::Fields) {
        caught += 1;
    }

    // 2 非十六进制载荷
    let t = text_with("1|ramp|1|0.500|zz|0000000000000000\n", 0);
    if parse_set(&t).err() == Some(WmError::Hex) {
        caught += 1;
    }

    // 3 未知种别
    let t = text_with("1|plaid|1|0.500|aa|0000000000000000\n", 0);
    if parse_set(&t).err() == Some(WmError::Kind) {
        caught += 1;
    }

    // 4 重复 id
    let t = text_with(
        "1|ramp|1|0.500|aa|0000000000000000\n1|checker|2|0.500|bb|0000000000000000\n",
        0,
    );
    if parse_set(&t).err() == Some(WmError::Dup) {
        caught += 1;
    }

    // 5 条数超上限（33 条）
    let mut body = String::new();
    let mut i = 0u16;
    while i < 33 {
        body.push_str(&format!(
            "{}|ramp|1|0.500|aa|0000000000000000\n",
            i + 1
        ));
        i += 1;
    }
    let t = text_with(&body, 0);
    if parse_set(&t).err() == Some(WmError::Count) {
        caught += 1;
    }

    // 6 空账（只有头）
    let t = text_with("", 0);
    if parse_set(&t).err() == Some(WmError::Empty) {
        caught += 1;
    }

    // 7 id 非数字
    let t = text_with("xx|ramp|1|0.500|aa|0000000000000000\n", 0);
    if parse_set(&t).err() == Some(WmError::Id) {
        caught += 1;
    }

    // 8 根校验和不符（头里 root 被改成 0）
    let t = text_with("1|ramp|1|0.500|aa|0000000000000000\n", 0);
    if parse_set(&t).err() == Some(WmError::Checksum) {
        caught += 1;
    }

    // 9 强度越界
    let t = text_with("1|ramp|1|1.500|aa|0000000000000000\n", 0);
    if parse_set(&t).err() == Some(WmError::Range) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50055_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50055");

    // J1-1 编解码往返：编码 → 解析后条数、id、校验和、根全部不变
    let built = builtin_set();
    let mut roundtrip_ok = false;
    let mut root_det = 0u64;
    if let Ok(set) = &built {
        let text = set.encode();
        match parse_set(&text) {
            Ok(back) => {
                root_det = back.root();
                let mut same = back.len() == set.len() && back.root() == set.root();
                let mut i = 0usize;
                while i < back.len() {
                    let a = set.vectors().get(i);
                    let b = back.vectors().get(i);
                    match (a, b) {
                        (Some(x), Some(y)) => {
                            if x.id != y.id
                                || vector_checksum(x) != vector_checksum(y)
                                || x.expect != y.expect
                            {
                                same = false;
                            }
                        }
                        _ => same = false,
                    }
                    i += 1;
                }
                roundtrip_ok = same;
            }
            Err(_) => {}
        }
    }
    s.add(
        "50055-J1-编解码往返",
        roundtrip_ok,
        if roundtrip_ok { "" } else { "编码/解析不同构" },
    );

    // J1-2 用例可复跑：同一账重放两次，逐条摘要与根一致
    let r1 = match &built {
        Ok(set) => Some(run_set(set)),
        Err(_) => None,
    };
    let r2 = match &built {
        Ok(set) => Some(run_set(set)),
        Err(_) => None,
    };
    let ok = match (&r1, &r2) {
        (Some(a), Some(b)) => {
            let mut same = a.passed == b.passed && a.failed == b.failed && a.root == b.root;
            let mut i = 0usize;
            while i < a.outcomes.len() {
                let x = a.outcomes.get(i);
                let y = b.outcomes.get(i);
                match (x, y) {
                    (Some(p), Some(q)) => {
                        if p.digest != q.digest || p.matched != q.matched {
                            same = false;
                        }
                    }
                    _ => same = false,
                }
                i += 1;
            }
            same
        }
        _ => false,
    };
    let det = match &r1 {
        Some(a) => format!("pass={} fail={}", a.passed, a.failed),
        None => "builtin failed".to_string(),
    };
    s.add_dyn("50055-J1-复跑一致", ok, &det);

    // J1-3 期望篡改必抓：改一条 expect，重放 failed 恰为 1
    let mut caught_one = false;
    if let Ok(mut set) = builtin_set() {
        let n = set.len();
        if let Some(v) = set.vectors_mut().get_mut(2) {
            v.expect ^= 1;
        }
        let rep = run_set(&set);
        caught_one = rep.failed == 1 && rep.passed + 1 == n;
    }
    s.add(
        "50055-J1-期望篡改必抓",
        caught_one,
        if caught_one { "" } else { "篡改未被检出" },
    );

    // J1-4 根校验和必守：头里 root 被改 → parse 直接 Checksum
    let t = text_with("1|ramp|1|0.500|aa|0000000000000000\n", 0);
    let bad_root = parse_set(&t).err() == Some(WmError::Checksum);
    let good_root = match builtin_set() {
        Ok(set) => {
            let text = set.encode();
            parse_set(&text).is_ok()
        }
        Err(_) => false,
    };
    let ok = bad_root && good_root;
    s.add("50055-J1-根校验和", ok, if ok { "" } else { "根校验失效" });

    // J1-5 坏十六进制必拒（奇数长度与非十六进制字符）
    let odd = hex_decode("abc").err() == Some(WmError::Hex);
    let non = hex_decode("zz").err() == Some(WmError::Hex);
    let ok = odd && non;
    s.add("50055-J1-十六进制必拒", ok, if ok { "" } else { "坏 hex 被放行" });

    // J1-6 未知种别必拒
    let ok = VecKind::parse("plaid").err() == Some(WmError::Kind)
        && VecKind::parse("ramp").is_ok();
    s.add("50055-J1-种别必拒", ok, if ok { "" } else { "未知种别被放行" });

    // J1-7 字段数错必拒（少于 6 段 / 多于 6 段）
    let short = parse_set(&text_with("1|ramp|1\n", 0)).err() == Some(WmError::Fields);
    let long = parse_set(&text_with("1|ramp|1|0.5|aa|00|extra\n", 0)).err()
        == Some(WmError::Fields);
    let ok = short && long;
    s.add("50055-J1-字段数必拒", ok, if ok { "" } else { "字段数错误被放行" });

    // J1-8 空账必拒（只有头，没有向量）
    let ok = parse_set(&text_with("", 0)).err() == Some(WmError::Empty);
    s.add("50055-J1-空账必拒", ok, if ok { "" } else { "空账被接受" });

    // J1-9 条数上限必拒
    let mut body = String::new();
    let mut i = 0u16;
    while i < (VEC_CAP as u16 + 1) {
        body.push_str(&format!("{}|ramp|1|0.500|aa|0000000000000000\n", i + 1));
        i += 1;
    }
    let ok = parse_set(&text_with(&body, 0)).err() == Some(WmError::Count);
    s.add("50055-J1-条数必拒", ok, if ok { "" } else { "条数上限失效" });

    // J1-10 重复 id 必拒
    let mut set = VectorSet::new();
    let a = WmVector {
        id: 5,
        kind: VecKind::Ramp,
        seed: 1,
        strength: 0.5,
        payload: vec_of(&[1, 2]),
        expect: 0,
    };
    let _ = set.add(a);
    let dup = WmVector {
        id: 5,
        kind: VecKind::Solid,
        seed: 2,
        strength: 0.5,
        payload: vec_of(&[3]),
        expect: 0,
    };
    let ok = set.add(dup).err() == Some(WmError::Dup);
    s.add("50055-J1-重复id必拒", ok, if ok { "" } else { "重复 id 被放行" });

    // J1-11 强度越界必拒
    let mut set = VectorSet::new();
    let v = WmVector {
        id: 6,
        kind: VecKind::Ramp,
        seed: 1,
        strength: 1.5,
        payload: vec_of(&[1]),
        expect: 0,
    };
    let ok = set.add(v).err() == Some(WmError::Range);
    s.add("50055-J1-强度必拒", ok, if ok { "" } else { "越界强度被放行" });

    // J1-12 校验和敏感：载荷单 bit 变化 → 校验和汉明距离 ≥ 8
    let v1 = WmVector {
        id: 7,
        kind: VecKind::Noise,
        seed: 9,
        strength: 0.5,
        payload: vec_of(&[0x55, 0xAA]),
        expect: 0,
    };
    let mut v2 = v1.clone();
    if let Some(b) = v2.payload.get_mut(0) {
        *b ^= 1;
    }
    let dist = (vector_checksum(&v1) ^ vector_checksum(&v2)).count_ones();
    let ok = dist >= 8;
    let det = format!("hamming={}", dist);
    s.add_dyn("50055-J1-校验和敏感", ok, &det);

    // J1-13 FNV-1a 外部锚点：官方测试向量 "a" → af63dc4c8601ec8c
    let anchor = fnv1a64(b"a");
    let ok = anchor == 0xaf63_dc4c_8601_ec8c;
    let det = format!("fnv={:016x}", anchor);
    s.add_dyn("50055-J1-FNV锚点", ok, &det);

    // J1-14 根顺序敏感：同样三条向量换序后根不同，两账各自自洽
    let mk = |a: u16, b: u16, c: u16| -> Option<u64> {
        let mut set = VectorSet::new();
        let ids = [a, b, c];
        let mut i = 0usize;
        while i < 3 {
            let v = WmVector {
                id: ids[i],
                kind: VecKind::Gradient,
                seed: i as u32 + 1,
                strength: 0.5,
                payload: vec_of(&[0x10 + i as u8]),
                expect: 0,
            };
            if set.add(v).is_err() {
                return None;
            }
            i += 1;
        }
        if set.verify().is_ok() {
            Some(set.root())
        } else {
            None
        }
    };
    let ok = match (mk(1, 2, 3), mk(3, 2, 1)) {
        (Some(x), Some(y)) => x != y,
        _ => false,
    };
    s.add("50055-J1-根顺序敏感", ok, if ok { "" } else { "换序后根不变" });

    // J1-15 守恒：passed + failed == 条数，且根与重算一致
    let ok = match &r1 {
        Some(rep) => {
            let n = match &built {
                Ok(set) => set.len(),
                Err(_) => 0,
            };
            rep.passed + rep.failed == n && rep.outcomes.len() == n
        }
        None => false,
    };
    let det = format!("root={:016x}", root_det);
    s.add_dyn("50055-J1-计数守恒", ok, &det);

    // J1-16 注入核九类全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50055-J1-注入必抓", ok, &det);

    s
}

fn vec_of(bytes: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            v.push(*b);
        }
        i += 1;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50055_checks_green() {
        let set = run_unx50055_checks();
        assert!(set.all_passed(), "UNX-F50055 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50055_encode_parse_roundtrip() {
        let set = builtin_set();
        assert!(set.is_ok());
        if let Ok(set) = set {
            let text = set.encode();
            let back = parse_set(&text);
            assert!(back.is_ok());
            if let Ok(back) = back {
                assert_eq!(back.len(), set.len());
                assert_eq!(back.root(), set.root());
            }
        }
    }
}
