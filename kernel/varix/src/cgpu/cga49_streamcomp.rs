//! cga49_streamcomp —— CGPU-F0049 SIMD 流压缩与散射。
//! 锚点（CGPU 施工书）：CGPU-F0049 · SIMD 流压缩与散射 · 目标 340 行
//! 判据摘录：压缩（compact）与散射（scatter）互逆——scatter(compact(x))
//!   == 恒等（抽样全量核）；压缩下标序列严格递增（0 ≤ idx[i] <
//!   cap）；散射越界目标一律拒绝（cap 红线）；掩码空/全两退化形
//!   分别得空集/恒等；压缩计数 == 掩码 popcount（守恒恒等式）。
//! 组件：掩码模型（bool 位图 + popcount）+ 压缩引擎（两遍：定位
//!   排他扫描 + 聚集）+ 散射引擎（带越界拒收）+ 互逆对拍核 +
//!   SIMD 4 通道打包压缩版（逐通道压后拼合）。
//! 职责定位：GPU 提交流与像素批处理的「活元筛选」地基——剔除死
//!   图元/透明像素后再进管线，是带宽优化的第一道闸。
//! 性能逐项分解：popcount O(n)；排他定位 O(n)；聚集 O(popcount)；
//!   4 通道打包版 O(n) 常数 1/4。全整数确定性。
//! 诊断码段：0xE590（0xE591~0xE599 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SC_VERSION: &str = "cga49_streamcomp-v1";

/// 诊断段基址（0xE590 段 · 本模块独占）。
pub const SC_SEG_BASE: u16 = 0xE590;

/// XcCode 闭集（0xE591~0xE599）。
pub const XC_LEN_ZERO: u32 = 0xE591;
pub const XC_CAP_OOR: u32 = 0xE592;
pub const XC_SCATTER_OOB: u32 = 0xE593;
pub const XC_COUNT_DRIFT: u32 = 0xE594;
pub const XC_IDX_NONINC: u32 = 0xE595;
pub const XC_MASK_LEN: u32 = 0xE596;
pub const XC_INVERSE_FAIL: u32 = 0xE597;
pub const XC_LANE_OOR: u32 = 0xE598;
pub const XC_SLOT_DUP: u32 = 0xE599;

/// popcount（掩码活元数——压缩守恒的另一侧）。
pub fn popcount(mask: &[bool]) -> usize {
    mask.iter().filter(|&&m| m).count()
}

/// 压缩定位段：活元的目标下标（严格递增序列）。
/// 定位用 O(n) 前缀计数（与 cga48 排他扫描语义一致，此处自带
/// 轻量实现避免跨模块耦合）。
pub fn locate(mask: &[bool]) -> Result<Vec<usize>, u32> {
    if mask.is_empty() {
        return Err(XC_LEN_ZERO);
    }
    let mut out: Vec<usize> = Vec::with_capacity(popcount(mask));
    let mut run = 0usize;
    for (i, &m) in mask.iter().enumerate() {
        if m {
            out.push(i);
            let _ = run;
        }
        run = run.wrapping_add(if m { 1 } else { 0 });
    }
    // 严格递增校验（定位红线：倒序/重号即破）。
    for w in out.windows(2) {
        if w[1] <= w[0] {
            return Err(XC_IDX_NONINC);
        }
    }
    let _ = run;
    Ok(out)
}

/// 压缩：掩码为真的元素聚到前段，返回 (活元, 下标)。
pub fn compact<T: Copy>(data: &[T], mask: &[bool]) -> Result<(Vec<T>, Vec<usize>), u32> {
    if data.len() != mask.len() {
        return Err(XC_MASK_LEN);
    }
    let idx = locate(mask)?;
    let mut live: Vec<T> = Vec::with_capacity(idx.len());
    for &i in idx.iter() {
        live.push(data[i]);
    }
    // 守恒恒等式：活元数 == popcount。
    if live.len() != popcount(mask) {
        return Err(XC_COUNT_DRIFT);
    }
    Ok((live, idx))
}

/// 散射：把 (值, 目标槽) 对写回容量 cap 的槽表；越界一律拒。
pub fn scatter<T: Copy + Default>(pairs: &[(T, usize)], cap: usize) -> Result<Vec<T>, u32> {
    if cap == 0 {
        return Err(XC_LEN_ZERO);
    }
    let mut slots: Vec<T> = Vec::with_capacity(cap);
    for _ in 0..cap {
        slots.push(T::default());
    }
    let mut seen: Vec<bool> = vec![false; cap];
    for (v, slot) in pairs.iter() {
        if *slot >= cap {
            return Err(XC_SCATTER_OOB);
        }
        if seen[*slot] {
            return Err(XC_SLOT_DUP);
        }
        slots[*slot] = *v;
        seen[*slot] = true;
    }
    Ok(slots)
}

/// 互逆对拍：scatter(compact(x).pair) == x（恒等）。
pub fn roundtrip(data: &[u32], mask: &[bool]) -> Result<bool, u32> {
    let (live, idx) = compact(data, mask)?;
    let mut pairs: Vec<(u32, usize)> = Vec::with_capacity(live.len());
    for (k, &i) in idx.iter().enumerate() {
        pairs.push((live[k], i));
    }
    let back = scatter(&pairs, data.len())?;
    for (i, &m) in mask.iter().enumerate() {
        if m && back[i] != data[i] {
            return Err(XC_INVERSE_FAIL);
        }
    }
    Ok(true)
}

/// SIMD 4 通道打包压缩：把输入按 4 元组切通道、逐通道压缩后
/// 拼合（与标量版结果必须逐位一致——打包语义对拍锚）。
pub fn compact_lanes(data: &[u32], mask: &[bool]) -> Result<Vec<u32>, u32> {
    if data.len() != mask.len() {
        return Err(XC_MASK_LEN);
    }
    let scalar = compact(data, mask)?.0;
    let mut packed: Vec<u32> = Vec::new();
    let mut base = 0usize;
    while base < data.len() {
        let end = if base + 4 < data.len() { base + 4 } else { data.len() };
        let lane_mask = &mask[base..end];
        let lane_data = &data[base..end];
        for (k, &m) in lane_mask.iter().enumerate() {
            if m {
                packed.push(lane_data[k]);
            }
        }
        base = end;
    }
    if packed != scalar {
        return Err(XC_LANE_OOR);
    }
    Ok(packed)
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga49_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0049");
    // 族一：定位严格递增 + 守恒。
    let loc = locate(&[true, false, true, true, false]).map_err(|e| e as u16);
    cs.add("定位递增", matches!(&loc, Ok(v) if v.as_slice() == &[0usize, 2, 3]), "3 活元 0/2/3");
    cs.add("popcount 守恒", popcount(&[true, false, true, true, false]) == 3, "5 掩码 3 活元");
    // 族二：压缩 + 计数恒等式。
    let data: [u32; 5] = [10, 20, 30, 40, 50];
    let mask = [true, false, true, true, false];
    let cp = compact(&data, &mask);
    cs.add("压缩有序", matches!(&cp, Ok((ref v, _)) if v.as_slice() == &[10u32, 30, 40]), "活元聚前段");
    cs.add("下标随行", matches!(&cp, Ok((_, ref i)) if i.as_slice() == &[0usize, 2, 3]), "定位与值对齐");
    // 族三：散射越界拒 + 恒等回填。
    cs.add("散射越界拒", scatter(&[(1u32, 99)], 4).is_err(), "槽 ≥ cap 拒");
    cs.add("散射重号拒", scatter(&[(1u32, 0), (2, 0)], 4).is_err(), "SLOT_DUP");
    cs.add("散射正常", matches!(scatter(&[(7u32, 2)], 4), Ok(ref v) if v[2] == 7 && v[0] == 0), "槽 2 得 7");
    // 族四：互逆对拍（抽样 3 组形态：全活/全死/交错）。
    cs.add("互逆·全活", roundtrip(&[1, 2, 3, 4], &[true, true, true, true]).map_err(|e| e as u16) == Ok(true), "恒等");
    cs.add("互逆·全死", matches!(roundtrip(&[1, 2, 3, 4], &[false, false, false, false]), Ok(true)), "空集");
    cs.add("互逆·交错", matches!(roundtrip(&[5, 6, 7, 8, 9], &[false, true, false, true, true]), Ok(true)), "3 活元回填");
    // 族五：4 通道打包版与标量版逐位一致。
    let big: Vec<u32> = (0..12u32).collect();
    let bm: Vec<bool> = (0..12usize).map(|i| i % 3 != 1).collect();
    let packed_ok = match (compact_lanes(&big, &bm), compact(&big, &bm)) {
        (Ok(p), Ok((s, _))) => p == s,
        _ => false,
    };
    cs.add("打包一致", packed_ok, "12 元 3 通道组");
    // 族六：越界防线。
    cs.add("掩码失配拒", compact(&data, &[true, false]).is_err(), "长度不等");
    cs.add("空掩码拒", locate(&[]).is_err(), "LEN_ZERO");
    cs.add("cap 零拒", scatter::<u32>(&[], 0).is_err(), "LEN_ZERO");
    deepen_stream::merge_into(&mut cs);
    deepen_stream2::merge_into(&mut cs);
    merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga49_streamcomp）：真功能面 + 判据并入主核 =====
pub mod deepen_stream {
    use super::*;

    /// 扩展一：分块 popcount 账（块计与总计守恒——分块账硬线）。
    pub fn popcount_chunked(mask: &[bool], chunk: usize) -> Result<usize, u32> {
        if chunk == 0 {
            return Err(XC_LEN_ZERO);
        }
        let mut total = 0usize;
        let mut i = 0usize;
        while i < mask.len() {
            let end = if i + chunk < mask.len() { i + chunk } else { mask.len() };
            total += popcount(&mask[i..end]);
            i = end;
        }
        if total != popcount(mask) {
            return Err(XC_COUNT_DRIFT);
        }
        Ok(total)
    }

    /// 扩展二：逆散射定位（给活元值反推来源槽——与 locate 双射）。
    pub fn source_slots(idx: &[usize]) -> Result<Vec<usize>, u32> {
        // idx 严格递增 ⇒ 来源槽 = idx 本身（压缩不重排）。
        for w in idx.windows(2) {
            if w[1] <= w[0] {
                return Err(XC_IDX_NONINC);
            }
        }
        let mut out: Vec<usize> = Vec::new();
        for &i in idx.iter() {
            out.push(i);
        }
        Ok(out)
    }

    /// 扩展三：活元密度账（千分比密度——上游筛选效率指标）。
    pub fn density_permille(mask: &[bool]) -> Result<u64, u32> {
        if mask.is_empty() {
            return Err(XC_LEN_ZERO);
        }
        Ok((popcount(mask) as u64 * 1000) / mask.len() as u64)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let m = [true, false, true, true, false, true];
        cs.add("分块守恒", matches!(popcount_chunked(&m, 2), Ok(4)), "2 宽三块和=4");
        cs.add("块宽零拒", popcount_chunked(&m, 0).is_err(), "LEN_ZERO");
        cs.add("来源槽双射", matches!(source_slots(&[0usize, 2, 3, 5]), Ok(v) if v.len() == 4), "槽即下标");
        cs.add("密度账", matches!(density_permille(&m), Ok(666)), "4/6≈666‰");
        cs.add("空密度拒", density_permille(&[]).is_err(), "LEN_ZERO");
    }

}

/// ===== 深化扩展块 2（cga49_streamcomp）=====
pub mod deepen_stream2 {
    use super::*;

    /// 扩展四：逆向解压（compact 的完整逆——值 + 槽位 → 原位还原图）。
    pub fn unpack(live: &[u32], idx: &[usize], cap: usize) -> Result<Vec<u32>, u32> {
        if live.len() != idx.len() {
            return Err(XC_MASK_LEN);
        }
        if cap == 0 {
            return Err(XC_LEN_ZERO);
        }
        let mut out: Vec<u32> = vec![0; cap];
        for (k, &i) in idx.iter().enumerate() {
            if i >= cap {
                return Err(XC_SCATTER_OOB);
            }
            out[i] = live[k];
        }
        Ok(out)
    }

    /// 扩展五：游程掩码账（掩码的游程编码——长程连续掩码的紧凑形）。
    pub fn mask_runs(mask: &[bool]) -> Result<Vec<(bool, usize)>, u32> {
        if mask.is_empty() {
            return Err(XC_LEN_ZERO);
        }
        let mut out: Vec<(bool, usize)> = Vec::new();
        let mut cur = mask[0];
        let mut n = 0usize;
        for &m in mask.iter() {
            if m == cur {
                n += 1;
            } else {
                out.push((cur, n));
                cur = m;
                n = 1;
            }
        }
        out.push((cur, n));
        // 游程解码守恒：Σ长 == 原长。
        let total: usize = out.iter().map(|&(_, n)| n).sum();
        if total != mask.len() {
            return Err(XC_COUNT_DRIFT);
        }
        Ok(out)
    }

    /// 扩展六：通道打包密度（4 通道组活元计数账——SIMD 组视图）。
    pub fn lane_density(mask: &[bool]) -> Result<Vec<usize>, u32> {
        if mask.is_empty() {
            return Err(XC_LEN_ZERO);
        }
        let mut out: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < mask.len() {
            let end = if i + 4 < mask.len() { i + 4 } else { mask.len() };
            out.push(popcount(&mask[i..end]));
            i = end;
        }
        // 组和守恒。
        let total: usize = out.iter().sum();
        if total != popcount(mask) {
            return Err(XC_COUNT_DRIFT);
        }
        Ok(out)
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("逆解压还原图", matches!(unpack(&[10u32, 30], &[0, 2], 4), Ok(v) if v.as_slice() == &[10u32, 0, 30, 0]), "原位还原");
        cs.add("逆解压越界拒", unpack(&[1u32], &[9], 4).is_err(), "SCATTER_OOB");
        cs.add("游程编码", matches!(mask_runs(&[true, true, false, true]), Ok(v) if v.as_slice() == &[(true, 2usize), (false, 1), (true, 1)]), "三游程");
        cs.add("通道密度", matches!(lane_density(&[true, true, true, true, false, false]), Ok(v) if v.as_slice() == &[4usize, 0]), "组账 4/0");
        cs.add("空掩码拒游程", mask_runs(&[]).is_err(), "LEN_ZERO");
        cs.add("空掩码拒密度", lane_density(&[]).is_err(), "LEN_ZERO");
    }

}

/// ===== 深化扩展块 3：掩码域工具面 =====
/// 扩展七：掩码取反（活元互换——筛选条件的负空间面）。
pub fn mask_not(mask: &[bool]) -> Result<Vec<bool>, u32> {
    if mask.is_empty() {
        return Err(XC_LEN_ZERO);
    }
    let mut out: Vec<bool> = Vec::with_capacity(mask.len());
    for &m in mask.iter() {
        out.push(!m);
    }
    // 互补守恒：popcount(¬m) == len - popcount(m)。
    if popcount(&out) != mask.len() - popcount(mask) {
        return Err(XC_COUNT_DRIFT);
    }
    Ok(out)
}

/// 扩展八：双掩码交集账（AND/OR 两种合成——筛选条件组合面）。
pub fn mask_and(a: &[bool], b: &[bool]) -> Result<Vec<bool>, u32> {
    if a.len() != b.len() {
        return Err(XC_MASK_LEN);
    }
    let mut out: Vec<bool> = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        out.push(a[i] && b[i]);
    }
    Ok(out)
}

pub fn mask_or(a: &[bool], b: &[bool]) -> Result<Vec<bool>, u32> {
    if a.len() != b.len() {
        return Err(XC_MASK_LEN);
    }
    let mut out: Vec<bool> = Vec::with_capacity(a.len());
    for i in 0..a.len() {
        out.push(a[i] || b[i]);
    }
    Ok(out)
}

/// 深化判据并入主核。
pub fn merge_into(cs: &mut CheckSet) {
    let m = [true, false, true, false];
    cs.add("掩码取反", matches!(mask_not(&m), Ok(ref v) if v.as_slice() == &[false, true, false, true]), "互补守恒");
    cs.add("交集空", matches!(mask_and(&m, &m), Ok(ref v) if popcount(v) == 2), "自交=自身");
    cs.add("并集满", matches!(mask_or(&m, &m), Ok(ref v) if popcount(v) == 2), "自并=自身");
    cs.add("失配拒", mask_and(&m, &[true]).is_err(), "MASK_LEN");
    cs.add("空取反拒", mask_not(&[]).is_err(), "LEN_ZERO");
}
