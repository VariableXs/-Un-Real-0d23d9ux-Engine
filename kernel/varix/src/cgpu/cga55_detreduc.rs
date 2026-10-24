//! cga55_detreduc —— CGPU-F0055 并行确定性归约。
//! 锚点（CGPU 施工书）：CGPU-F0055 · 并行确定性归约 · 目标 320 行
//! 判据摘录：树形归约（成对合并）与顺序左折叠结果逐位一致
//!   （浮点禁用，整数算术天然可结合——一致性硬线）；归约单位元
//!   正确性（Sum→0、Min→u32::MAX、Max→0、Xor→0）；空输入显性
//!   拒绝（不静默回单位元——上层必须知道空集没有意义）；分块
//!   归约（chunk × 树）与全量树归约一致（分块不变式）。
//! 组件：Op 闭集四族 + 单层树归约引擎 + 分块归约引擎（块内树 +
//!   块间树两级）+ 单位元表 + 一致性对拍核 + 交换律/结合律抽样
//!   核（固定语料）。
//! 职责定位：GPU 侧所有统计量（和/极值/校验）的确定性地基——
//!   浮点归约的顺序不定性是 GPU 管线重放差异的头号来源，本模块
//!   用纯整数域把「归约结果可重放」钉死。
//! 性能逐项分解：树归约 O(n) 次合并、O(log n) 级深；分块版常数
//!   减半（块内 cache 友好）。全整数确定性。
//! 诊断码段：0xE5B0（0xE5B1~0xE5B9 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DR_VERSION: &str = "cga55_detreduc-v1";

/// 诊断段基址（0xE5B0 段 · 本模块独占）。
pub const DR_SEG_BASE: u16 = 0xE5B0;

/// XcCode 闭集（0xE5B1~0xE5B9）。
pub const XC_EMPTY: u32 = 0xE5B1;
pub const XC_OP_OOR: u32 = 0xE5B2;
pub const XC_CHUNK_ZERO: u32 = 0xE5B3;
pub const XC_PAIR_DRIFT: u32 = 0xE5B4;
pub const XC_IDENT_BAD: u32 = 0xE5B5;
pub const XC_OVERFLOW: u32 = 0xE5B6;
pub const XC_MONO_BAD: u32 = 0xE5B7;
pub const XC_ASSOC_FAIL: u32 = 0xE5B8;
pub const XC_LEN_MISMATCH: u32 = 0xE5B9;

/// 归约算子四闭集。
pub const OP_SUM: u8 = 0;
pub const OP_MIN: u8 = 1;
pub const OP_MAX: u8 = 2;
pub const OP_XOR: u8 = 3;

/// 单位元表（闭集路由）。
pub fn identity(op: u8) -> Result<u64, u32> {
    match op {
        OP_SUM => Ok(0),
        OP_MIN => Ok(u64::MAX),
        OP_MAX => Ok(0),
        OP_XOR => Ok(0),
        _ => Err(XC_OP_OOR),
    }
}

/// 二元合并（wrapping 域——溢出在 GPU 统计里是红线，由上层
/// 预算控制，本层保持确定性 wrapping 语义并留显性探测）。
pub fn combine(a: u64, b: u64, op: u8) -> Result<u64, u32> {
    match op {
        OP_SUM => Ok(a.wrapping_add(b)),
        OP_MIN => Ok(if a < b { a } else { b }),
        OP_MAX => Ok(if a > b { a } else { b }),
        OP_XOR => Ok(a ^ b),
        _ => Err(XC_OP_OOR),
    }
}

/// 树形归约（成对合并，级别内 i 与 i+1 合并、步距倍增）。
pub fn tree_reduce(data: &[u64], op: u8) -> Result<u64, u32> {
    let id = identity(op)?;
    if data.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut cur: Vec<u64> = Vec::with_capacity(data.len());
    for &v in data.iter() {
        cur.push(v);
    }
    let mut d = 1usize;
    while d < cur.len() {
        let mut next: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < cur.len() {
            if i + d < cur.len() {
                next.push(combine(cur[i], cur[i + d], op)?);
                i += 2 * d;
            } else {
                next.push(cur[i]);
                i += d;
            }
        }
        cur = next;
        d *= 2;
    }
    let _ = id;
    Ok(cur[0])
}

/// 顺序左折叠（对拍参照）。
pub fn seq_reduce(data: &[u64], op: u8) -> Result<u64, u32> {
    let mut acc = identity(op)?;
    if data.is_empty() {
        return Err(XC_EMPTY);
    }
    for &v in data.iter() {
        acc = combine(acc, v, op)?;
    }
    Ok(acc)
}

/// 一致性对拍：树形 == 顺序（整数域硬线）。
pub fn consistency(data: &[u64], op: u8) -> Result<(), u32> {
    let a = tree_reduce(data, op)?;
    let b = seq_reduce(data, op)?;
    if a != b {
        return Err(XC_PAIR_DRIFT);
    }
    Ok(())
}

/// 分块归约：块内树归约 + 块间树归约（两级），与全量树归约一致。
pub fn chunked_reduce(data: &[u64], chunk: usize, op: u8) -> Result<u64, u32> {
    if chunk == 0 {
        return Err(XC_CHUNK_ZERO);
    }
    if data.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut partials: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let end = if i + chunk < data.len() { i + chunk } else { data.len() };
        partials.push(tree_reduce(&data[i..end], op)?);
        i = end;
    }
    tree_reduce(&partials, op)
}

/// 分块不变式：任意块宽（1、2、3、7、64）结果 == 全量树归约。
pub fn chunk_invariant(data: &[u64], op: u8) -> Result<(), u32> {
    let whole = tree_reduce(data, op)?;
    for &c in [1usize, 2, 3, 7, 64].iter() {
        if chunked_reduce(data, c, op)? != whole {
            return Err(XC_CHUNK_ZERO + 100);
        }
    }
    Ok(())
}

/// 结合律抽样核：固定语料上 (a∘b)∘c == a∘(b∘c)。
pub fn assoc_sample(op: u8) -> Result<(), u32> {
    let corpus: [u64; 8] = [1, 2, 3, 5, 8, 13, 21, 34];
    for &a in corpus.iter() {
        for &b in corpus.iter() {
            for &c in corpus.iter() {
                let l = combine(combine(a, b, op)?, c, op)?;
                let r = combine(a, combine(b, c, op)?, op)?;
                if l != r {
                    return Err(XC_ASSOC_FAIL);
                }
            }
        }
    }
    Ok(())
}

/// 交换律抽样核（Sum/Min/Max/Xor 四族均交换）。
pub fn comm_sample(op: u8) -> Result<(), u32> {
    let corpus: [u64; 6] = [3, 7, 11, 17, 23, 31];
    for &a in corpus.iter() {
        for &b in corpus.iter() {
            if combine(a, b, op)? != combine(b, a, op)? {
                return Err(XC_MONO_BAD);
            }
        }
    }
    Ok(())
}

/// 单位元守恒：x ∘ id == x（四族全查）。
pub fn identity_conserve(x: u64, op: u8) -> Result<(), u32> {
    let id = identity(op)?;
    if combine(x, id, op)? != x {
        return Err(XC_IDENT_BAD);
    }
    Ok(())
}

/// 溢出显性探测（Sum 专用：wrapping 环绕置溢出旗，供上层预算）。
pub fn sum_overflow_probe(data: &[u64]) -> Result<(u64, bool), u32> {
    if data.is_empty() {
        return Err(XC_EMPTY);
    }
    let mut acc: u64 = 0;
    let mut ovf = false;
    for &v in data.iter() {
        let (r, o1) = acc.overflowing_add(v);
        if o1 {
            ovf = true;
        }
        acc = r;
    }
    Ok((acc, ovf))
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga55_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0055");
    let corpus: Vec<u64> = vec![5, 3, 9, 1, 7, 2, 8, 4, 6];
    // 族一：四族树归约 == 顺序折叠。
    for (name, op) in [("求和", OP_SUM), ("取小", OP_MIN), ("取大", OP_MAX), ("异或", OP_XOR)].iter() {
        cs.add(name, consistency(&corpus, *op).is_ok(), "树形==顺序");
    }
    // 族二：值对拍（手算锚）。
    cs.add("和=45", matches!(tree_reduce(&corpus, OP_SUM), Ok(45)), "1+..+9 子集");
    cs.add("min=1", matches!(tree_reduce(&corpus, OP_MIN), Ok(1)), "语料极小");
    cs.add("max=9", matches!(tree_reduce(&corpus, OP_MAX), Ok(9)), "语料极大");
    // 族三：分块不变式（5 档块宽）。
    cs.add("分块不变", chunk_invariant(&corpus, OP_SUM).is_ok(), "块宽 1/2/3/7/64");
    // 族四：单位元守恒。
    cs.add("单位元守恒", {
        let mut ok = true;
        for op in [OP_SUM, OP_MIN, OP_MAX, OP_XOR].iter() {
            if identity_conserve(42, *op).is_err() {
                ok = false;
            }
        }
        ok
    }, "四族 x∘id==x");
    // 族五：结合律 + 交换律抽样核（全语料对全语料）。
    cs.add("结合律抽样", assoc_sample(OP_SUM).is_ok(), "8³ 三元组");
    cs.add("交换律抽样", comm_sample(OP_MIN).is_ok(), "6² 二元组");
    cs.add("Xor 结合", assoc_sample(OP_XOR).is_ok(), "异或域");
    // 族六：空输入显性拒绝 + 越界码拒绝。
    cs.add("空输入拒", tree_reduce(&[], OP_SUM).is_err(), "EMPTY 显性");
    cs.add("空折叠拒", seq_reduce(&[], OP_MAX).is_err(), "EMPTY 显性");
    cs.add("算子越界拒", combine(1, 1, 9).is_err(), "OP_OOR");
    cs.add("单位元越界拒", identity(9).is_err(), "OP_OOR");
    cs.add("块宽零拒", chunked_reduce(&corpus, 0, OP_SUM).is_err(), "CHUNK_ZERO");
    // 族七：溢出显性探测。
    cs.add("无溢出", matches!(sum_overflow_probe(&[1, 2, 3]), Ok((6, false))), "和=6 无旗");
    cs.add("溢出探测", matches!(sum_overflow_probe(&[u64::MAX, 1]), Ok((0, true))), "环绕+置旗");
    deepen_reduc::merge_into(&mut cs);
    deepen_reduc2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga55_detreduc）：真功能面 + 判据并入主核 =====
pub mod deepen_reduc {
    use super::*;

    /// 扩展一：加权归约（Σv×w / Σw——权重和零显性拒绝）。
    pub fn weighted_sum(values: &[u64], weights: &[u64]) -> Result<u64, u32> {
        if values.len() != weights.len() || values.is_empty() {
            return Err(XC_LEN_MISMATCH);
        }
        let wsum: u128 = weights.iter().map(|&w| w as u128).sum();
        if wsum == 0 {
            return Err(XC_MONO_BAD);
        }
        let acc: u128 = values.iter().zip(weights.iter()).map(|(&v, &w)| v as u128 * w as u128).sum();
        Ok((acc / wsum) as u64)
    }

    /// 扩展二：极值定位账（max 归约附带首现下标——定位面）。
    pub fn argmax(data: &[u64]) -> Result<(usize, u64), u32> {
        if data.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut bi = 0usize;
        for (i, &v) in data.iter().enumerate() {
            if v > data[bi] {
                bi = i;
            }
        }
        Ok((bi, data[bi]))
    }

    /// 扩展三：饱和归约（Sum 饱和到 u64::MAX 不环绕——与 wrapping
    /// 语义双轨，上层按需选择）。
    pub fn sat_sum(data: &[u64]) -> Result<u64, u32> {
        if data.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut acc: u64 = 0;
        for &v in data.iter() {
            acc = acc.saturating_add(v);
        }
        Ok(acc)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("加权归约", matches!(weighted_sum(&[10, 20, 30], &[1, 1, 2]), Ok(22)), "(10+20+60)/4");
        cs.add("权重零拒", weighted_sum(&[1, 2], &[0, 0]).is_err(), "Σw=0 显性");
        cs.add("argmax 首现", matches!(argmax(&[3, 9, 9, 1]), Ok((1, 9))), "首现下标");
        cs.add("argmax 空拒", argmax(&[]).is_err(), "EMPTY");
        cs.add("饱和和", matches!(sat_sum(&[u64::MAX, 1, 1]), Ok(u64::MAX)), "钳满不环");
    }

}

/// ===== 深化扩展块 2（cga55_detreduc）=====
pub mod deepen_reduc2 {
    use super::*;

    /// 扩展四：均值归约（和/长——除零显性拒绝）。
    pub fn mean(data: &[u64]) -> Result<u64, u32> {
        if data.is_empty() {
            return Err(XC_EMPTY);
        }
        let s = tree_reduce(data, OP_SUM)?;
        Ok(s / data.len() as u64)
    }

    /// 扩展五：方差矩账（Σ(x-μ)² 整数矩——离散步数域）。
    pub fn variance_int(data: &[u64]) -> Result<u64, u32> {
        if data.is_empty() {
            return Err(XC_EMPTY);
        }
        let mu = mean(data)?;
        let mut acc: u64 = 0;
        for &v in data.iter() {
            let d = if v > mu { v - mu } else { mu - v };
            acc = acc.saturating_add(d * d);
        }
        Ok(acc / data.len() as u64)
    }

    /// 扩展六：前缀归约账（归约的中间级快照——树形级联输出）。
    pub fn cascade_levels(data: &[u64], op: u8) -> Result<Vec<Vec<u64>>, u32> {
        if data.is_empty() {
            return Err(XC_EMPTY);
        }
        let mut cur: Vec<u64> = Vec::new();
        for &v in data.iter() {
            cur.push(v);
        }
        let mut out: Vec<Vec<u64>> = Vec::new();
        out.push(cur.clone());
        while cur.len() > 1 {
            let mut next: Vec<u64> = Vec::new();
            let mut i = 0usize;
            while i + 1 < cur.len() {
                next.push(combine(cur[i], cur[i + 1], op)?);
                i += 2;
            }
            if cur.len() % 2 == 1 {
                next.push(cur[cur.len() - 1]);
            }
            out.push(next.clone());
            cur = next;
        }
        Ok(out)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let d: Vec<u64> = vec![2, 4, 6, 8];
        cs.add("均值", matches!(mean(&d), Ok(5)), "20/4");
        cs.add("均值空拒", mean(&[]).is_err(), "EMPTY");
        cs.add("方差矩", matches!(variance_int(&d), Ok(5)), "(9+1+1+9)/4");
        cs.add("级联首级", matches!(cascade_levels(&d, OP_SUM), Ok(v) if v[0].as_slice() == &[2u64, 4, 6, 8]), "快照 0 级");
        cs.add("级联末级单值", matches!(cascade_levels(&d, OP_SUM), Ok(v) if v[v.len() - 1].as_slice() == &[20u64]), "终值 20");
        cs.add("级联空拒", cascade_levels(&[], OP_SUM).is_err(), "EMPTY");
    }

}
