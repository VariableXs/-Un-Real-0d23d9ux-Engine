//! UNX-F50011 色彩 fuzz 面（坏 profile 注入不崩）（0x42d80 段）
//!
//! 锚点：UNX-F50011-J1 fuzz ×10^4 零崩；判据锚定 §AI-63 §5。
//!
//! 数据段 SEG_BASE = 0x42d80，诊断码闭集为 Base+1..Base+7。
//! 「零崩」在 no_std 里不能靠 catch_unwind 证明，只能靠**结构**证明：解析器
//! 每一步都返回 Result，变异器自身不做越界写，于是 10^4 次变异只可能落在
//! Ok/Err 两条路径上。本模块把这两条路径都记账：
//! * Err 路径：按 `ColorError::code() - 0x42c40` 落直方图，类别数不足即判「浅」；
//! * Ok 路径：再用 `strict_ok()` 做独立后验（行列式、逆矩阵残差、白点、TRC），
//!   后验不过记为 violation——这就是「静默接受坏 profile」的检出点。
//!
//! 结构清单：
//! * `XorShift64` 确定性伪随机源（同种子逐位复现，fuzz 可回放）；
//! * `Mutation` 九类变异策略（位翻/改字节/截断/追加/清零/超大字段/交换/复制/原样）；
//! * `seed_profile()` 最小合法 ICC 构造器（5 标签：rXYZ/gXYZ/bXYZ/rTRC/desc）；
//! * `run_fuzz()` 变异性调度器 + `FuzzReport` 台账（直方图/策略命中/长度界/违例）；
//! * 注入核 `injection_core()`：八类手写坏样本逐类断言确定诊断码。
//!
//! 错误纪律：内核路径零 panic；所有写入都经 `get_mut()` 与边界判断，
//! 变异结果硬截断在 `MAX_INPUT`，解析交给 F50001 的零 panic 解析器。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::fabsf;

use super::unx50001_gamutmap::{
    parse_profile, ColorError, IccProfile, Trc, SEG_BASE as COLOR_SEG_BASE,
};

/// 诊断段（0x42d80 独占 0x20）。
pub const SEG_BASE: u32 = 0x42d80;

pub const E_EMPTY: u16 = 1;
pub const E_BADSEED: u16 = 2;
pub const E_BUDGET: u16 = 3;
pub const E_UNSTABLE: u16 = 4;
pub const E_THIN: u16 = 5;
pub const E_STRATEGY: u16 = 6;
pub const E_OVERFLOW: u16 = 7;

/// fuzz 面错误闭集。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuzzError {
    Empty,
    BadSeed,
    Budget,
    Unstable,
    Thin,
    Strategy,
    Overflow,
}

impl FuzzError {
    pub fn code(self) -> u32 {
        let n = match self {
            FuzzError::Empty => E_EMPTY,
            FuzzError::BadSeed => E_BADSEED,
            FuzzError::Budget => E_BUDGET,
            FuzzError::Unstable => E_UNSTABLE,
            FuzzError::Thin => E_THIN,
            FuzzError::Strategy => E_STRATEGY,
            FuzzError::Overflow => E_OVERFLOW,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            FuzzError::Empty => "seed profile empty",
            FuzzError::BadSeed => "seed profile does not parse",
            FuzzError::Budget => "iteration budget contradicted",
            FuzzError::Unstable => "post-validation violation (silent accept)",
            FuzzError::Thin => "error histogram too thin",
            FuzzError::Strategy => "some mutation strategy never fired",
            FuzzError::Overflow => "mutated buffer exceeds input cap",
        }
    }
}

/// J1 预算：10^4 次。
pub const FUZZ_ITERS: usize = 10_000;
/// 直方图至少覆盖的错误类别数。
pub const MIN_CLASSES: usize = 4;
/// 单次变异后的输入上限（保护解析器的时间与栈）。
pub const MAX_INPUT: usize = 1024;

/// xorshift64（13/7/17）。种子 0 被替换成黄金比常量，避免全零不动点。
#[derive(Clone, Copy, Debug)]
pub struct XorShift64 {
    s: u64,
}

impl XorShift64 {
    pub const fn new(seed: u64) -> XorShift64 {
        XorShift64 {
            s: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.s;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.s = x;
        x
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    /// 无偏下限其实需要拒绝采样；fuzz 只要确定性，取模即可。
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next_u32() % n
        }
    }
}

/// 九类变异。原样（None）保留一条「合法输入必须仍可解析」的对照路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutation {
    None,
    BitFlip,
    ByteSet,
    Truncate,
    Extend,
    ZeroRegion,
    HugeField,
    Swap,
    Duplicate,
}

impl Mutation {
    pub const ALL: [Mutation; 9] = [
        Mutation::None,
        Mutation::BitFlip,
        Mutation::ByteSet,
        Mutation::Truncate,
        Mutation::Extend,
        Mutation::ZeroRegion,
        Mutation::HugeField,
        Mutation::Swap,
        Mutation::Duplicate,
    ];

    pub fn code(self) -> u8 {
        match self {
            Mutation::None => 0,
            Mutation::BitFlip => 1,
            Mutation::ByteSet => 2,
            Mutation::Truncate => 3,
            Mutation::Extend => 4,
            Mutation::ZeroRegion => 5,
            Mutation::HugeField => 6,
            Mutation::Swap => 7,
            Mutation::Duplicate => 8,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mutation::None => "none",
            Mutation::BitFlip => "bit-flip",
            Mutation::ByteSet => "byte-set",
            Mutation::Truncate => "truncate",
            Mutation::Extend => "extend",
            Mutation::ZeroRegion => "zero-region",
            Mutation::HugeField => "huge-field",
            Mutation::Swap => "swap",
            Mutation::Duplicate => "duplicate",
        }
    }
}

// ---------------------------------------------------------------------------
// 字节写入原语（全部带边界检查，宁可少写也不越界）
// ---------------------------------------------------------------------------

fn patch_u32(v: &mut Vec<u8>, at: usize, val: u32) {
    let bytes = [
        (val >> 24) as u8,
        (val >> 16) as u8,
        (val >> 8) as u8,
        val as u8,
    ];
    let mut i = 0usize;
    while i < 4 {
        if let (Some(dst), Some(src)) = (v.get_mut(at + i), bytes.get(i)) {
            *dst = *src;
        }
        i += 1;
    }
}

fn patch_u16(v: &mut Vec<u8>, at: usize, val: u16) {
    let bytes = [(val >> 8) as u8, val as u8];
    let mut i = 0usize;
    while i < 2 {
        if let (Some(dst), Some(src)) = (v.get_mut(at + i), bytes.get(i)) {
            *dst = *src;
        }
        i += 1;
    }
}

fn set_bytes(v: &mut Vec<u8>, at: usize, bytes: &[u8]) {
    let mut i = 0usize;
    while i < bytes.len() {
        if let (Some(dst), Some(src)) = (v.get_mut(at + i), bytes.get(i)) {
            *dst = *src;
        }
        i += 1;
    }
}

/// s15Fixed16 写入（颜色通道用）。
fn write_xyz(v: &mut Vec<u8>, at: usize, x: f32, y: f32, z: f32) {
    let vals = [x, y, z];
    let mut i = 0usize;
    while i < 3 {
        if let Some(val) = vals.get(i) {
            let raw = (*val * 65536.0) as i32;
            patch_u32(v, at + i * 4, raw as u32);
        }
        i += 1;
    }
}

fn zeros(n: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(n);
    let mut i = 0usize;
    while i < n {
        v.push(0u8);
        i += 1;
    }
    v
}

/// 最小合法 ICC profile：头 128 字节 + 5 标签表（60 字节）+ 数据区 98 字节。
/// 布局：rXYZ@192 / gXYZ@212 / bXYZ@232 / rTRC@252 / desc@266，总长 290。
pub fn seed_profile() -> Vec<u8> {
    const TOTAL: usize = 290;
    let mut v = zeros(TOTAL);
    patch_u32(&mut v, 0, TOTAL as u32);
    set_bytes(&mut v, 8, &[0x04, 0x30, 0x00, 0x00]); // v4.3
    set_bytes(&mut v, 12, b"mntr");
    set_bytes(&mut v, 16, b"RGB ");
    set_bytes(&mut v, 20, b"XYZ ");
    set_bytes(&mut v, 36, b"acsp");
    patch_u32(&mut v, 64, 0);
    write_xyz(&mut v, 68, 0.9642, 1.0, 0.8252);
    patch_u32(&mut v, 128, 5);

    let tags = [
        (*b"rXYZ", 192u32, 20u32),
        (*b"gXYZ", 212, 20),
        (*b"bXYZ", 232, 20),
        (*b"rTRC", 252, 14),
        (*b"desc", 266, 24),
    ];
    let mut i = 0usize;
    while i < tags.len() {
        if let Some(t) = tags.get(i) {
            let e = 132 + i * 12;
            set_bytes(&mut v, e, &t.0);
            patch_u32(&mut v, e + 4, t.1);
            patch_u32(&mut v, e + 8, t.2);
        }
        i += 1;
    }

    let colorants = [
        (192usize, 0.4360747f32, 0.2225045f32, 0.0139322f32),
        (212, 0.3850649, 0.7168786, 0.0971045),
        (232, 0.1430804, 0.0606169, 0.7141733),
    ];
    let mut j = 0usize;
    while j < colorants.len() {
        if let Some(c) = colorants.get(j) {
            set_bytes(&mut v, c.0, b"XYZ ");
            write_xyz(&mut v, c.0 + 8, c.1, c.2, c.3);
        }
        j += 1;
    }

    set_bytes(&mut v, 252, b"curv");
    patch_u32(&mut v, 260, 1);
    patch_u16(&mut v, 264, 563); // 2.2 × 256
    set_bytes(&mut v, 266, b"desc");
    let text = b"UNX fuzz seed";
    patch_u32(&mut v, 274, (text.len() + 1) as u32);
    set_bytes(&mut v, 278, text);
    v
}

/// 独立后验（解析器输出自洽性）：解析成功不等于字段自洽。
/// 这里核对解析器文档承诺的四条不变量——头部尺寸/tag 数在闭集内、白点为正、
/// 色料矩阵可逆、反矩阵乘积≈单位阵、TRC 参数为正。任一条不成立都说明
/// 解析器放出了坏结果，必须计入 violation。
pub fn strict_ok(p: &IccProfile) -> bool {
    if p.tag_count > 128 || p.size_bytes < 128 {
        return false;
    }
    if p.size_bytes as usize > MAX_INPUT {
        return false;
    }
    if !(p.white.x > 0.0 && p.white.y > 0.0 && p.white.z > 0.0) {
        return false;
    }
    let d = p.rgb_to_xyz.det();
    if d != d || fabsf(d) < 1e-8 {
        return false;
    }
    let prod = p.rgb_to_xyz.mul(&p.xyz_to_rgb);
    let mut i = 0usize;
    while i < 3 {
        let mut j = 0usize;
        while j < 3 {
            let want = if i == j { 1.0f32 } else { 0.0f32 };
            let got = match prod.m.get(i).and_then(|row| row.get(j)) {
                Some(v) => *v,
                None => return false,
            };
            if fabsf(got - want) > 1e-2 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    match &p.trc {
        Trc::Srgb => true,
        Trc::Gamma(g) => *g > 0.0,
        Trc::Table(t) => !t.is_empty(),
    }
}

/// 九类变异器。所有取放都带边界检查；结果硬截断到 `MAX_INPUT`。
pub fn mutate(seed: &[u8], m: Mutation, rng: &mut XorShift64) -> Vec<u8> {
    let mut out = Vec::with_capacity(seed.len() + 32);
    let mut i = 0usize;
    while i < seed.len() {
        if let Some(b) = seed.get(i) {
            out.push(*b);
        }
        i += 1;
    }
    if out.is_empty() {
        return out;
    }
    let len = out.len();
    match m {
        Mutation::None => {}
        Mutation::BitFlip => {
            let rounds = 1 + rng.below(4);
            let mut k = 0u32;
            while k < rounds {
                let pos = rng.below(len as u32) as usize;
                let bit = rng.below(8) as u32;
                if let Some(c) = out.get_mut(pos) {
                    *c ^= 1u8 << bit;
                }
                k += 1;
            }
        }
        Mutation::ByteSet => {
            let pos = rng.below(len as u32) as usize;
            let val = rng.below(256) as u8;
            if let Some(c) = out.get_mut(pos) {
                *c = val;
            }
        }
        Mutation::Truncate => {
            let keep = rng.below(len as u32) as usize;
            out.truncate(keep);
        }
        Mutation::Extend => {
            let add = 1 + rng.below(64) as usize;
            let mut k = 0usize;
            while k < add {
                out.push(rng.below(256) as u8);
                k += 1;
            }
        }
        Mutation::ZeroRegion => {
            let pos = rng.below(len as u32) as usize;
            let span = 1 + rng.below(32) as usize;
            let mut k = 0usize;
            while k < span {
                if let Some(c) = out.get_mut(pos + k) {
                    *c = 0;
                }
                k += 1;
            }
        }
        Mutation::HugeField => {
            if len >= 4 {
                let slots = (len - 3) as u32;
                let pos = rng.below(slots) as usize;
                patch_u32(&mut out, pos, u32::MAX);
            } else {
                let pos = rng.below(len as u32) as usize;
                if let Some(c) = out.get_mut(pos) {
                    *c = 0xff;
                }
            }
        }
        Mutation::Swap => {
            let a = rng.below(len as u32) as usize;
            let b = rng.below(len as u32) as usize;
            let va = match out.get(a) {
                Some(v) => *v,
                None => 0,
            };
            let vb = match out.get(b) {
                Some(v) => *v,
                None => 0,
            };
            if let Some(c) = out.get_mut(a) {
                *c = vb;
            }
            if let Some(c) = out.get_mut(b) {
                *c = va;
            }
        }
        Mutation::Duplicate => {
            let pos = rng.below(len as u32) as usize;
            let mut chunk = [0u8; 4];
            let mut k = 0usize;
            while k < 4 {
                if let Some(v) = out.get(pos + k) {
                    if let Some(c) = chunk.get_mut(k) {
                        *c = *v;
                    }
                }
                k += 1;
            }
            let mut q = 0usize;
            while q < 4 {
                if let Some(v) = chunk.get(q) {
                    out.push(*v);
                }
                q += 1;
            }
        }
    }
    if out.len() > MAX_INPUT {
        out.truncate(MAX_INPUT);
    }
    out
}

/// fuzz 台账。`hist` 下标即 `ColorError::code() - 0x42c40`（1..13）。
#[derive(Clone, Debug, PartialEq)]
pub struct FuzzReport {
    pub iterations: u64,
    pub accepted: u64,
    pub rejected: u64,
    pub violations: u64,
    pub hist: [u64; 16],
    pub strategies: [u64; 9],
    pub max_len: usize,
    pub min_len: usize,
    pub first_error: Option<u32>,
}

impl FuzzReport {
    fn blank() -> FuzzReport {
        FuzzReport {
            iterations: 0,
            accepted: 0,
            rejected: 0,
            violations: 0,
            hist: [0u64; 16],
            strategies: [0u64; 9],
            max_len: 0,
            min_len: usize::MAX,
            first_error: None,
        }
    }

    /// 直方图覆盖到的错误类别数（只看 1..13 的闭集区）。
    pub fn distinct_errors(&self) -> usize {
        let mut n = 0usize;
        let mut i = 1usize;
        while i < 14 {
            if let Some(c) = self.hist.get(i) {
                if *c > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn hist_total(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.hist.len() {
            if let Some(c) = self.hist.get(i) {
                sum += *c;
            }
            i += 1;
        }
        sum
    }

    pub fn strategies_hit(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.strategies.len() {
            if let Some(c) = self.strategies.get(i) {
                if *c > 0 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn crash_free(&self) -> bool {
        self.violations == 0
    }

    pub fn summary(&self) -> String {
        format!(
            "色彩fuzz iters={} accept={} reject={} classes={} strategies={} viol={} len=[{},{}]",
            self.iterations,
            self.accepted,
            self.rejected,
            self.distinct_errors(),
            self.strategies_hit(),
            self.violations,
            self.min_len,
            self.max_len
        )
    }
}

/// 变异性调度器：每轮均匀抽一种策略，变异后交给解析器。
/// 解析成功再做独立后验；失败按诊断码落直方图。两条路径都记账。
pub fn run_fuzz(seed: u64, iters: usize) -> FuzzReport {
    let base = seed_profile();
    let mut rng = XorShift64::new(seed);
    let mut rep = FuzzReport::blank();
    let mut n = 0usize;
    while n < iters {
        let pick = rng.below(Mutation::ALL.len() as u32) as usize;
        let m = match Mutation::ALL.get(pick) {
            Some(v) => *v,
            None => Mutation::None,
        };
        let buf = mutate(&base, m, &mut rng);
        if buf.len() > MAX_INPUT {
            rep.violations += 1;
        }
        match parse_profile(&buf) {
            Ok(p) => {
                rep.accepted += 1;
                if !strict_ok(&p) {
                    rep.violations += 1;
                }
            }
            Err(e) => {
                rep.rejected += 1;
                let idx = (e.code() - COLOR_SEG_BASE) as usize;
                if idx < rep.hist.len() {
                    if let Some(c) = rep.hist.get_mut(idx) {
                        *c += 1;
                    }
                } else {
                    rep.violations += 1;
                }
                if rep.first_error.is_none() {
                    rep.first_error = Some(e.code());
                }
            }
        }
        if let Some(c) = rep.strategies.get_mut(pick) {
            *c += 1;
        }
        if buf.len() > rep.max_len {
            rep.max_len = buf.len();
        }
        if buf.len() < rep.min_len {
            rep.min_len = buf.len();
        }
        rep.iterations += 1;
        n += 1;
    }
    rep
}

/// J1 闸门：跑满 10^4 次并核对四项不变量（零违例、类别不浅、双路径都出现、
/// 九策略全覆盖）。任一不满足返回显式错误，绝不静默降级。
pub fn fuzz_guard(seed: u64) -> Result<FuzzReport, FuzzError> {
    let base = seed_profile();
    if base.is_empty() {
        return Err(FuzzError::Empty);
    }
    if parse_profile(&base).is_err() {
        return Err(FuzzError::BadSeed);
    }
    let rep = run_fuzz(seed, FUZZ_ITERS);
    if rep.violations > 0 {
        return Err(FuzzError::Unstable);
    }
    if rep.distinct_errors() < MIN_CLASSES {
        return Err(FuzzError::Thin);
    }
    if rep.accepted == 0 || rep.rejected == 0 {
        return Err(FuzzError::Budget);
    }
    if rep.iterations != FUZZ_ITERS as u64 {
        return Err(FuzzError::Budget);
    }
    if rep.strategies_hit() != Mutation::ALL.len() {
        return Err(FuzzError::Strategy);
    }
    if rep.max_len > MAX_INPUT {
        return Err(FuzzError::Overflow);
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------
// 注入核
// ---------------------------------------------------------------------------

fn poison_magic(v: &mut Vec<u8>) {
    set_bytes(v, 36, &[0, 0, 0, 0]);
}

fn poison_size(v: &mut Vec<u8>) {
    patch_u32(v, 0, 40_000);
}

fn poison_tagcount(v: &mut Vec<u8>) {
    patch_u32(v, 128, 9_999);
}

fn poison_tag_offset(v: &mut Vec<u8>) {
    patch_u32(v, 136, 9_999);
}

fn poison_white(v: &mut Vec<u8>) {
    write_xyz(v, 68, 0.0, 0.0, 0.0);
}

fn poison_singular(v: &mut Vec<u8>) {
    // 把 gXYZ/bXYZ 写成与 rXYZ 相同的色料 → 三列线性相关，det = 0
    write_xyz(v, 220, 0.4360747, 0.2225045, 0.0139322);
    write_xyz(v, 240, 0.4360747, 0.2225045, 0.0139322);
}

/// 八类手写坏样本 → 命中数。每条都要求确定诊断码，不允许「任意 Err」。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;

    if parse_profile(&[]).err() == Some(ColorError::Empty) {
        caught += 1;
    }
    if parse_profile(&zeros(64)).err() == Some(ColorError::Short) {
        caught += 1;
    }

    let mut magic = seed_profile();
    poison_magic(&mut magic);
    if parse_profile(&magic).err() == Some(ColorError::Magic) {
        caught += 1;
    }

    let mut trunc = seed_profile();
    poison_size(&mut trunc);
    if parse_profile(&trunc).err() == Some(ColorError::Truncated) {
        caught += 1;
    }

    let mut count = seed_profile();
    poison_tagcount(&mut count);
    if parse_profile(&count).err() == Some(ColorError::TagCount) {
        caught += 1;
    }

    let mut off = seed_profile();
    poison_tag_offset(&mut off);
    if parse_profile(&off).err() == Some(ColorError::TagOffset) {
        caught += 1;
    }

    let mut white = seed_profile();
    poison_white(&mut white);
    if parse_profile(&white).err() == Some(ColorError::White) {
        caught += 1;
    }

    let mut sing = seed_profile();
    poison_singular(&mut sing);
    if parse_profile(&sing).err() == Some(ColorError::Singular) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50011_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50011");

    // J1-1 种子 profile 自身必须合法且通过独立后验
    let seed = seed_profile();
    let parsed = parse_profile(&seed);
    let ok = match &parsed {
        Ok(p) => p.tag_count == 5 && strict_ok(p) && p.desc.contains("UNX"),
        Err(_) => false,
    };
    let det = format!("len={} tags={}", seed.len(), match &parsed {
        Ok(p) => p.tag_count,
        Err(_) => 0,
    });
    s.add_dyn("50011-J1-种子合法", ok, &det);

    // J1-2 10^4 次零违例（零崩 + 零静默接受）
    let guard = fuzz_guard(0x5EED_5001);
    let (ok, iters, acc, rej, viol) = match &guard {
        Ok(r) => (
            r.iterations == FUZZ_ITERS as u64 && r.crash_free() && r.accepted + r.rejected == r.iterations,
            r.iterations,
            r.accepted,
            r.rejected,
            r.violations,
        ),
        Err(_) => (false, 0, 0, 0, 0),
    };
    let det = format!("iters={} acc={} rej={} viol={}", iters, acc, rej, viol);
    s.add_dyn("50011-J1-fuzz零崩", ok, &det);

    // J1-3 直方图覆盖 ≥4 类错误，且总数等于拒绝数
    let (classes, hist_total, rejected) = match &guard {
        Ok(r) => (r.distinct_errors(), r.hist_total(), r.rejected),
        Err(_) => (0, 0, 0),
    };
    let ok = classes >= MIN_CLASSES && hist_total == rejected && rejected > 0;
    let det = format!("classes={} hist={} rej={}", classes, hist_total, rejected);
    s.add_dyn("50011-J1-类别够厚", ok, &det);

    // J1-4 九类变异策略全部发火
    let hit = match &guard {
        Ok(r) => r.strategies_hit(),
        Err(_) => 0,
    };
    let ok = hit == Mutation::ALL.len();
    let det = format!("strategies={}/{}", hit, Mutation::ALL.len());
    s.add_dyn("50011-J1-策略覆盖", ok, &det);

    // J1-5 确定性：同种子两跑逐字段一致（fuzz 可回放）
    let a = run_fuzz(0xABCD_1234, 500);
    let b = run_fuzz(0xABCD_1234, 500);
    let ok = a == b && a.iterations == 500 && a.crash_free();
    let det = format!("acc={} rej={} viol={}", a.accepted, a.rejected, a.violations);
    s.add_dyn("50011-J1-可回放", ok, &det);

    // J1-6 不同种子给出不同分布（证明确实在变异而不是空转）
    let c = run_fuzz(0x1111_2222, 500);
    let ok = c.hist != a.hist || c.accepted != a.accepted;
    s.add("50011-J1-种子有效", ok, if ok { "" } else { "不同种子同分布" });

    // J1-7 变异长度硬上限：200 次全部 ≤ MAX_INPUT，且至少一次触到截断路径
    let mut rng = XorShift64::new(0xFEED_FACE);
    let mut over = 0usize;
    let mut truncated = 0usize;
    let mut k = 0usize;
    while k < 200 {
        let pick = rng.below(Mutation::ALL.len() as u32) as usize;
        let m = match Mutation::ALL.get(pick) {
            Some(v) => *v,
            None => Mutation::None,
        };
        let out = mutate(&seed, m, &mut rng);
        if out.len() > MAX_INPUT {
            over += 1;
        }
        if out.len() < seed.len() {
            truncated += 1;
        }
        k += 1;
    }
    let ok = over == 0 && truncated > 0;
    let det = format!("over={} truncated={}", over, truncated);
    s.add_dyn("50011-J1-长度受限", ok, &det);

    // J1-8 空/短输入显式拒绝，不返回伪 profile
    let ok = parse_profile(&[]).err() == Some(ColorError::Empty)
        && parse_profile(&zeros(63)).err() == Some(ColorError::Short)
        && parse_profile(&zeros(64)).err() == Some(ColorError::Short);
    s.add("50011-J1-空短输入", ok, if ok { "" } else { "空/短输入未拒" });

    // J1-9 变异体不产生「可解析但后验不过」的样本（violations 已为 0 的独立复核）
    let mut silent = 0usize;
    let mut rng2 = XorShift64::new(0x0BAD_5EED);
    let mut q = 0usize;
    while q < 300 {
        let pick = rng2.below(Mutation::ALL.len() as u32) as usize;
        let m = match Mutation::ALL.get(pick) {
            Some(v) => *v,
            None => Mutation::None,
        };
        let out = mutate(&seed, m, &mut rng2);
        if let Ok(p) = parse_profile(&out) {
            if !strict_ok(&p) {
                silent += 1;
            }
        }
        q += 1;
    }
    let ok = silent == 0;
    let det = format!("silent_accept={}", silent);
    s.add_dyn("50011-J1-后验闭环", ok, &det);

    // J1-10 首错码落在 0x42c40 色彩段内
    let first = match &guard {
        Ok(r) => r.first_error,
        Err(_) => None,
    };
    let ok = match first {
        Some(code) => code > COLOR_SEG_BASE && code <= COLOR_SEG_BASE + 13,
        None => false,
    };
    let det = match first {
        Some(code) => format!("first=0x{:05x}", code),
        None => String::new(),
    };
    s.add_dyn("50011-J1-首错在段内", ok, &det);

    // J1-11 双路径都出现：接受（原样）与拒绝都有样本
    let (acc, rej) = match &guard {
        Ok(r) => (r.accepted, r.rejected),
        Err(_) => (0, 0),
    };
    let ok = acc > 0 && rej > 0;
    let det = format!("acc={} rej={}", acc, rej);
    s.add_dyn("50011-J1-双路径", ok, &det);

    // J1-12 八类手写坏样本逐个确定命中
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50011-J1-注入必抓", ok, &det);

    // J1-13 摘要非空且字段齐备
    let text = match &guard {
        Ok(r) => r.summary(),
        Err(_) => String::new(),
    };
    let ok = !text.is_empty()
        && text.contains("iters=10000")
        && text.contains("classes=")
        && text.contains("viol=0");
    s.add("50011-J1-摘要非空", ok, if ok { "" } else { "摘要缺失" });

    // J1-14 种子 0 被替换为常量，不出现全零退化
    let mut z = XorShift64::new(0);
    let v1 = z.next_u64();
    let v2 = z.next_u64();
    let ok = v1 != 0 && v2 != 0 && v1 != v2;
    s.add("50011-J1-零种子防护", ok, if ok { "" } else { "零种子退化" });

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50011_checks_green() {
        let set = run_unx50011_checks();
        assert!(set.all_passed(), "UNX-F50011 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50011_seed_roundtrip() {
        let seed = seed_profile();
        assert_eq!(seed.len(), 290);
        let p = match parse_profile(&seed) {
            Ok(v) => v,
            Err(_) => panic!("seed profile must parse"),
        };
        assert_eq!(p.tag_count, 5);
        assert!(strict_ok(&p));
    }

    #[test]
    fn unx50011_prng_replay() {
        let mut a = XorShift64::new(42);
        let mut b = XorShift64::new(42);
        let mut i = 0;
        while i < 32 {
            assert_eq!(a.next_u64(), b.next_u64());
            i += 1;
        }
    }

    #[test]
    fn unx50011_mutation_bounded() {
        let seed = seed_profile();
        let mut rng = XorShift64::new(7);
        let mut i = 0usize;
        while i < Mutation::ALL.len() {
            if let Some(m) = Mutation::ALL.get(i) {
                let out = mutate(&seed, *m, &mut rng);
                assert!(out.len() <= MAX_INPUT);
            }
            i += 1;
        }
    }

    #[test]
    fn unx50011_error_codes_in_segment() {
        assert_eq!(FuzzError::Empty.code(), SEG_BASE + 1);
        assert_eq!(FuzzError::Overflow.code(), SEG_BASE + 7);
    }
}
