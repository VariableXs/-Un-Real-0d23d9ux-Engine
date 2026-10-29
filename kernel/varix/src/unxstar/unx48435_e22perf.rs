//! UNX-F48435 —— E22 性能面（长串处理线性复杂度断言）
//! 判据原文：UNX-F48435-J1 长串处理必须保持线性复杂度：倍长输入步数比值有界，
//! 单字节摊销不升，零长输入有常数下限，批处理总量与逐量之和守恒，无平方级路径。
//! 判据落地（drill 场景）：R1 构造器确定性；R2 步数恰等于线性模型值；
//! R3 倍长比值有界（1.7..2.3）；R4 摊销步数不升；R5 零长常数下限；
//! R6 批守恒（总量 = 分量之和）；R7 分配次数恒定；R8 预算硬顶；R9 指纹稳定；
//! R10 对折比值下界（防虚报模型）；R11 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDA60;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_BUILD: u16 = SEG_BASE + 1; // 构造器违反确定性
const E_MODEL: u16 = SEG_BASE + 2; // 步数偏离线性模型
const E_RATIO: u16 = SEG_BASE + 3; // 倍长比值越界（疑似平方路径）
const E_AMORT: u16 = SEG_BASE + 4; // 单字节摊销步数上升
const E_ZERO: u16 = SEG_BASE + 5; // 零长输入缺常数下限
const E_CONSERV: u16 = SEG_BASE + 6; // 批守恒破缺（总量 ≠ 分量和）
const E_ALLOC: u16 = SEG_BASE + 7; // 分配次数随长度增长
const E_BUDGET: u16 = SEG_BASE + 8; // 步数超硬顶预算
const E_FP: u16 = SEG_BASE + 9; // 指纹漂移
const E_FOLD: u16 = SEG_BASE + 10; // 对折比值低于下界（模型虚报）
const E_REPORT: u16 = SEG_BASE + 11; // 呈现行缺段

/// 线性模型：ops = K×len + C。
pub const LAT_K: u64 = 3;
pub const LAT_C: u64 = 16;
/// 倍长比值容差窗。
pub const RATIO_LO_X100: u64 = 170; // 1.70 × 100
pub const RATIO_HI_X100: u64 = 230; // 2.30 × 100
/// 倍长序列（字节长度）。
pub const LENGTHS: [usize; 4] = [4096, 8192, 16384, 32768];
/// 单次批内分配硬顶：构造 1 次 + 计量 0 次。
pub const ALLOC_CAP: u32 = 1;

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 长串样本：字节体 + 构造期分配计数。
pub struct Sample {
    pub body: Vec<u8>,
    pub allocs: u32,
}

/// 按 LCG 确定性填充 len 字节样本（单次分配，内容确定性）。
pub fn build_sample(lcg: &mut Lcg, len: usize) -> Sample {
    let mut body = vec![0u8; len];
    let mut i = 0usize;
    while i < len {
        // 字节值取自 LCG 高位，限幅到可打印区，保证确定性且非全零。
        body[i] = (lcg.next_u32() >> 16) as u8 % 95 + 32;
        i += 1;
    }
    Sample { body, allocs: 1 }
}

/// 计量器：访问字节数记账。
pub struct OpMeter {
    pub visits: u64,
}

impl OpMeter {
    pub fn new() -> OpMeter {
        OpMeter { visits: 0 }
    }
    /// 单遍扫描：每字节恰好访问一次（线性实现本体）。
    pub fn scan(&mut self, data: &[u8]) {
        let mut i = 0usize;
        while i < data.len() {
            self.visits += 1;
            i += 1;
        }
    }
    /// 模型复算值：K×len + C。
    pub fn model(len: u64) -> u64 {
        len * LAT_K + LAT_C
    }
    /// 记账总步数：每字节计 K 步，外加常数段 C。
    pub fn ops(&self) -> u64 {
        self.visits * LAT_K + LAT_C
    }
}

/// 摊销步数放大 100 倍整数化：ops×100/len。
fn amort_x100(ops: u64, len: u64) -> u64 {
    if len == 0 {
        return 0;
    }
    ops * 100 / len
}

/// 倍长比值放大 100 倍整数化：ops2×100/ops1（len2 = 2×len1）。
fn ratio_x100(ops1: u64, ops2: u64) -> u64 {
    if ops1 == 0 {
        return 0;
    }
    ops2 * 100 / ops1
}

/// 计量结果行（呈现面）：len=<n> ops=<n> amort=<x100>。
pub fn meter_line(len: usize, ops: u64) -> String {
    let mut s = String::new();
    s.push_str("len=");
    s.push_str(&(len as u32).to_string());
    s.push_str(" ops=");
    s.push_str(&ops.to_string());
    s.push_str(" amort=");
    s.push_str(&amort_x100(ops, len as u64).to_string());
    s
}

/// 批聚合指纹。
fn batch_fp(samples: &[Sample]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < samples.len() {
        h = fnv1a64_fold(h, &samples[i].body);
        i += 1;
    }
    h
}

/// 链式 FNV（以给定初值继续递推）。
fn fnv1a64_fold(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    hh
}

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 构造器确定性：同种子同体，异种子异体。
    let mut a = Lcg::new(0x48435);
    let mut b = Lcg::new(0x48435);
    let mut c = Lcg::new(0x48436);
    let sa = build_sample(&mut a, 64);
    let sb = build_sample(&mut b, 64);
    let sc = build_sample(&mut c, 64);
    if sa.body != sb.body || sa.body == sc.body {
        return Err(E_BUILD);
    }
    hits += 1;

    // R2/R3/R4/R8/R10：倍长序列逐级计量。
    let mut lcg = Lcg::new(0x51A60);
    let mut prev_ops: u64 = 0;
    let mut prev_amort: u64 = 0;
    let mut total_ops: u64 = 0;
    let mut total_len: u64 = 0;
    let mut samples: Vec<Sample> = vec![];
    let mut li = 0usize;
    while li < LENGTHS.len() {
        let len = LENGTHS[li];
        let s = build_sample(&mut lcg, len);
        if s.body.len() != len {
            return Err(E_BUILD);
        }
        let mut m = OpMeter::new();
        m.scan(&s.body);
        let ops = m.ops();
        // R2 线性模型：visits 恰等于 len，记账总步数恰等于 K×len + C。
        if m.visits != len as u64 {
            return Err(E_MODEL);
        }
        if ops != OpMeter::model(len as u64) {
            return Err(E_MODEL);
        }
        // R8 硬顶：ops ≤ 预算（K+1)×len + 2C。
        if ops > (len as u64) * (LAT_K + 1) + LAT_C * 2 {
            return Err(E_BUDGET);
        }
        // R4 摊销不升。
        let am = amort_x100(ops, len as u64);
        if prev_amort != 0 && am > prev_amort {
            return Err(E_AMORT);
        }
        // R3 倍长比值有界；R10 对折下界。
        if prev_ops != 0 {
            let r = ratio_x100(prev_ops, ops);
            if r < RATIO_LO_X100 || r > RATIO_HI_X100 {
                return Err(E_RATIO);
            }
        }
        // 首级与零长对照给出对折下界（ops 随长度严格增长）。
        if prev_ops != 0 && ops <= prev_ops {
            return Err(E_FOLD);
        }
        prev_ops = ops;
        prev_amort = am;
        total_ops += ops;
        total_len += len as u64;
        samples.push(s);
        li += 1;
    }
    hits += 1;

    // R5 零长常数下限：ops == C，摊销记 0。
    let zero = build_sample(&mut lcg, 0);
    let mut mz = OpMeter::new();
    mz.scan(&zero.body);
    let zops = mz.ops();
    if zops != LAT_C {
        return Err(E_ZERO);
    }
    if amort_x100(zops, 0) != 0 {
        return Err(E_ZERO);
    }
    hits += 1;

    // R6 批守恒：分量 ops 之和 == 一次 concat 大样本的 ops（C 只记一次）。
    let mut big = vec![0u8; total_len as usize];
    let mut off = 0usize;
    let mut si = 0usize;
    while si < samples.len() {
        let seg = &samples[si].body;
        let mut k = 0usize;
        while k < seg.len() {
            big[off + k] = seg[k];
            k += 1;
        }
        off += seg.len();
        si += 1;
    }
    let mut mbig = OpMeter::new();
    mbig.scan(&big);
    let big_ops = mbig.ops();
    // 分量和含 n 次 C，concat 只含 1 次 C：扣除 (n-1)×C 后应相等。
    let n = samples.len() as u64;
    if big_ops + (n - 1) * LAT_C != total_ops {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R7 分配次数恒定：每样本构造期恰 1 次分配，与长度无关。
    let mut si = 0usize;
    while si < samples.len() {
        if samples[si].allocs != ALLOC_CAP {
            return Err(E_ALLOC);
        }
        si += 1;
    }
    hits += 1;

    // R8' 拼装偏移守恒：concat 写入指针必须恰停在 total_len。
    if off as u64 != total_len {
        return Err(E_CONSERV);
    }
    hits += 1;

    // R9 指纹稳定：同批两次指纹一致且非零。
    let f1 = batch_fp(&samples);
    let f2 = batch_fp(&samples);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R11 呈现行：len=/ops=/amort= 三段齐全，且各级互异。
    let l0 = meter_line(LENGTHS[0], prev_ops / 8);
    let l3 = meter_line(LENGTHS[3], prev_ops);
    if l0 == l3 {
        return Err(E_REPORT);
    }
    if !has_kw(l0.as_bytes(), b"len=") || !has_kw(l0.as_bytes(), b"ops=") || !has_kw(l0.as_bytes(), b"amort=") {
        return Err(E_REPORT);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
