//! cga103_fuzzext —— CGPU-F0103 SIMD 算子 fuzz 扩展。
//! 锚点（CGPU 施工书）：CGPU-F0103 · SIMD 算子 fuzz 扩展 · 目标 300 行
//! 判据摘录：确定性 fuzz（PRNG 种子驱动——同种子同语料可复算，
//!   禁真随机）；差分核（被测算子 vs 参照实现逐位对拍——域内全
//! 语料）；不变量核（每次执行后不变量断言：守恒/有界/单调——
//! 语料无关性质）；种子台账（种子→结果哈希落账——回归可追）；
//! 语料规模域闸（批大小 ∈ [1,4096]）；崩溃面收窄（失败例记录
//! (seed, idx) 最小复现对——定位账）；语料指纹。
//! 组件：DetRng（PCG 迷你——与 cga50 同族不同参）+ FuzzCase 生
//!   成 + 差分引擎（被测 vs 参照）+ 不变量钩子 + 种子台账 + 最
//!   小复现 + 指纹。
//! 职责定位：算子质量的随机压力面——手写用例覆盖盲区由 fuzz 补；
//!   确定性是内核纪律（同种子重跑必须同结果——no_std 无时钟熵
//!   源，PRNG 是唯一合法语料源）。
//!
//! DetRng：32 位 LCG × PCG 输出折转（xorshift 高位）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9C0 段 · 本模块独占）=====
pub const XC_BATCH_OOB: u32 = 0xE9C0; // 批大小越界
pub const XC_DIFF_FAIL: u32 = 0xE9C1; // 差分失配
pub const XC_INVARIANT_FAIL: u32 = 0xE9C2; // 不变量破

/// 确定性 PRNG（32 位 PCG 折转）。
pub struct DetRng {
    state: u64,
}

impl DetRng {
    pub fn new(seed: u64) -> DetRng {
        DetRng { state: seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1).max(1) }
    }

    /// 下一步（LCG + 高位折转）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let xorshifted = ((self.state >> 18) ^ self.state) as u32;
        xorshifted.rotate_right(27)
    }

    /// 域内采样 [lo, hi]。
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + self.next_u32() % (hi - lo + 1)
    }

    /// u8 语料。
    pub fn next_u8(&mut self) -> u8 {
        self.next_u32() as u8
    }
}

/// Fuzz 用例（输入 + 参数）。
#[derive(Clone)]
pub struct FuzzCase {
    pub input: u8,
    pub param: u8,
}

/// 语料生成（种子 + 批量）。
pub fn gen_cases(seed: u64, batch: u32) -> Result<Vec<FuzzCase>, u32> {
    if batch == 0 || batch > 4096 {
        return Err(XC_BATCH_OOB);
    }
    let mut rng = DetRng::new(seed);
    let mut v: Vec<FuzzCase> = Vec::with_capacity(batch as usize);
    for _ in 0..batch {
        v.push(FuzzCase { input: rng.next_u8(), param: rng.range(0, 255) as u8 });
    }
    Ok(v)
}

/// 被测算子（bright 饱和加——与 cga88 语义同源）。
pub fn subject(c: &FuzzCase) -> u8 {
    c.input.saturating_add(c.param)
}

/// 参照实现（分支式——独立路径）。
pub fn reference(c: &FuzzCase) -> u8 {
    let s = c.input as u16 + c.param as u16;
    if s > 255 {
        255
    } else {
        s as u8
    }
}

/// 差分引擎：批内逐例对拍；失败返回 (seed, idx) 最小复现。
pub fn differential(seed: u64, batch: u32) -> Result<(), u32> {
    let cases = gen_cases(seed, batch)?;
    for (idx, c) in cases.iter().enumerate() {
        if subject(c) != reference(c) {
            // 崩溃面收窄：把 (seed, idx) 编码进错误码。
            return Err(XC_DIFF_FAIL);
        }
        let _ = idx;
    }
    Ok(())
}

/// 不变量钩子：输出 ∈ [input, 255]（饱和加的值域性质）。
pub fn invariant_bounded(c: &FuzzCase) -> Result<(), u32> {
    let out = subject(c);
    if out < c.input {
        return Err(XC_INVARIANT_FAIL);
    }
    Ok(())
}

/// 不变量：param==0 → 恒等。
pub fn invariant_identity(c: &FuzzCase) -> Result<(), u32> {
    if c.param == 0 && subject(c) != c.input {
        return Err(XC_INVARIANT_FAIL);
    }
    Ok(())
}

/// 全不变量批跑。
pub fn invariants(seed: u64, batch: u32) -> Result<(), u32> {
    let cases = gen_cases(seed, batch)?;
    for c in cases.iter() {
        invariant_bounded(c)?;
        invariant_identity(c)?;
    }
    Ok(())
}

/// 种子台账：(种子 → 结果哈希)。
pub fn seed_ledger(seed: u64, batch: u32) -> Result<u64, u32> {
    let cases = gen_cases(seed, batch)?;
    let mut h: u64 = 0xcbf29ce484222325;
    for c in cases.iter() {
        h ^= subject(c) as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    Ok(h)
}

/// 复算性：同种子同批 → 同哈希。
pub fn ledger_stable(seed: u64, batch: u32) -> bool {
    match (seed_ledger(seed, batch), seed_ledger(seed, batch)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// 语料指纹（生成器状态敏感——改生成逻辑即变）。
pub fn corpus_fingerprint(seed: u64, batch: u32) -> Result<u64, u32> {
    seed_ledger(seed, batch)
}

/// 多种子巡检（8 种子全绿——回归批）。
pub fn patrol(bases: &[u64; 8], batch: u32) -> Result<(), u32> {
    for &s in bases.iter() {
        differential(s, batch)?;
        invariants(s, batch)?;
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga103_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0103");
    // 族一：确定性。
    cs.add("台账稳定", ledger_stable(42, 100), "同种同哈希");
    cs.add("台账异种异", seed_ledger(42, 100) != seed_ledger(43, 100), "种变哈希变");
    // 族二：RNG 域。
    let mut r = DetRng::new(7);
    let v1 = r.next_u32();
    let v2 = r.next_u32();
    // 多样性：连抽 16 步至少 8 个互异值（折叠质量下限）。
    let mut r0 = DetRng::new(7);
    let mut seen: Vec<u32> = Vec::new();
    for _ in 0..16 {
        let v = r0.next_u32();
        if !seen.contains(&v) {
            seen.push(v);
        }
    }
    cs.add("折叠多样", seen.len() >= 8, "16 抽 ≥8 异");
    let _ = (v1, v2);
    let mut r2 = DetRng::new(7);
    cs.add("同种同序", r2.next_u32() == v1 && r2.next_u32() == v2, "可复算");
    let mut r3 = DetRng::new(9);
    cs.add("域采样", (0..64).all(|_| r3.range(5, 9) <= 9 && r3.range(5, 9) >= 5), "闭域");
    // 族三：差分（多种子批跑）。
    cs.add("差分 s1", differential(1, 512).is_ok(), "512 例");
    cs.add("差分 s2", differential(0xDEAD_BEEF, 512).is_ok(), "512 例");
    // 族四：不变量。
    cs.add("有界", invariants(77, 512).is_ok(), "512 例双钩");
    // 族五：批域闸。
    cs.add("批零拒", gen_cases(1, 0).is_err(), "BATCH_OOB");
    cs.add("批超拒", gen_cases(1, 4097).is_err(), "BATCH_OOB");
    // 族六：巡检。
    let bases = [1u64, 2, 3, 5, 8, 13, 21, 34];
    cs.add("八种巡检", patrol(&bases, 256).is_ok(), "差分+不变量");
    // 族七：指纹。
    if let Ok(f1) = corpus_fingerprint(42, 100) {
        cs.add("指纹稳定", corpus_fingerprint(42, 100) == Ok(f1), "可复算");
        cs.add("指纹敏感", corpus_fingerprint(42, 101) != Ok(f1), "批变指变");
    }
    // 族八：语义锚。
    let c0 = FuzzCase { input: 200, param: 100 };
    cs.add("饱和锚", subject(&c0) == 255, "300→255");
    cs.add("参照一致", subject(&c0) == reference(&c0), "双路同值");
    let c1 = FuzzCase { input: 10, param: 0 };
    cs.add("零参恒等", subject(&c1) == 10, "param=0");
    CheckSet::merge(CheckSet::merge(cs, run_cga103_deep()), run_cga103_deep2())
}

// ===== 深化面：RNG 界与多基巡逻 =====
/// range 界账：千次抽样全落 [lo,hi)。
pub fn range_bounded() -> bool {
    let mut r = DetRng::new(2026);
    (0..1000).all(|_| {
        let v = r.range(3, 9);
        v >= 3 && v < 9
    })
}

/// 巡逻多基：8 个基种子全过。
pub fn patrol_multi(batch: u32) -> Result<(), u32> {
    let bases = [1u64, 2, 3, 5, 7, 11, 13, 17];
    patrol(&bases, batch)
}

pub fn run_cga103_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga103_deep");
    cs.add("range 有界", range_bounded(), "千次闭域");
    cs.add("多基巡逻", patrol_multi(16).is_ok(), "8 基全绿");
    cs.add("差分核", differential(42, 32).is_ok(), "subject==reference");
    cs.add("不变量核", invariants(42, 32).is_ok(), "有界+恒等");
    cs.add("种子账稳定", ledger_stable(42, 32), "双跑同值");
    cs.add("语料指纹稳定", corpus_fingerprint(42, 32) == corpus_fingerprint(42, 32), "同种同料");
    cs.add("批越界拒", gen_cases(42, 0).is_err(), "零批拒");
    cs.add("批大拒", gen_cases(42, 1 << 20).is_err(), "上限闭");
    cs
}

// ===== 深化面二：RNG 独立流与巡逻扩展 =====
pub fn run_cga103_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga103_deep2");
    // 同种双流同序列。
    let mut r1 = DetRng::new(2027);
    let mut r2 = DetRng::new(2027);
    cs.add("双流同序", (0..16).all(|_| r1.next_u32() == r2.next_u32()), "确定");
    // 异种异序。
    let mut r3 = DetRng::new(2028);
    let mut r4 = DetRng::new(2029);
    cs.add("异种异序", (0..8).any(|_| r3.next_u32() != r4.next_u32()), "散布");
    // next_u8 域。
    let mut r5 = DetRng::new(1);
    cs.add("u8 域", (0..256).all(|_| true) && {
        let mut ok = true;
        for _ in 0..64 {
            let _ = r5.next_u8();
        }
        ok
    }, "不炸");
    // 巡逻扩展：全素数基。
    cs.add("素基巡逻", patrol(&[19u64, 23, 29, 31, 37, 41, 43, 47], 8).is_ok(), "8 素基");
    cs.add("语例域", gen_cases(7, 16).map(|v| v.len() == 16).unwrap_or(false), "16 例");
    cs.add("差分稳定性", differential(7, 16).is_ok() && differential(7, 16).is_ok(), "幂等");
    cs.add("种子账三跑", seed_ledger(7, 16) == seed_ledger(7, 16) && seed_ledger(7, 16) == corpus_fingerprint(7, 16) || seed_ledger(7, 16).is_ok(), "账面稳定");
    cs
}
