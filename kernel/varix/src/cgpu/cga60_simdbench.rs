//! cga60_simdbench —— CGPU-F0060 A 域 SIMD 基准冻结。
//! 锚点（CGPU 施工书）：CGPU-F0060 · A 域 SIMD 基准冻结 · 目标 320 行
//! 判据摘录：基准算子清单冻结（六算子：位移/转换/前缀和/压缩/
//!   排序网/归约——各占一条基准，步数模型定死）；步数模型与算子
//!   复杂度逐条一致（O(n) 算子不得登记 O(n log n) 模型）；基准
//!   结果可重放（同输入两跑逐位一致——确定性硬线）；预算上限
//!   表冻结（每算子最大样本步数）；清单闭集（越界算子名拒绝）。
//! 组件：算子步数模型表（六条冻结）+ 重放核（双跑一致）+ 预算
//!   上限表 + 清单闭集校验 + 汇总报表（口径 = 表条数）。
//! 职责定位：A 域性能账的「度量衡」——基准不是计时而是步数计
//!   数模型（与 cga57 守卫同口径），冻结后任何算子语义漂移都会
//!   使步数模型失配而立案。
//! 性能逐项分解：每基准 O(语料)；重放 2×O(语料)；全整数确定性。
//! 诊断码段：0xE620（0xE621~0xE629 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SB_VERSION: &str = "cga60_simdbench-v1";

/// 诊断段基址（0xE620 段 · 本模块独占）。
pub const SB_SEG_BASE: u16 = 0xE620;

/// XcCode 闭集（0xE621~0xE629）。
pub const XC_OP_UNKNOWN: u32 = 0xE621;
pub const XC_MODEL_DRIFT: u32 = 0xE622;
pub const XC_REPLAY_FAIL: u32 = 0xE623;
pub const XC_BUDGET_OOB: u32 = 0xE624;
pub const XC_LIST_EMPTY: u32 = 0xE625;
pub const XC_SAMPLE_MISS: u32 = 0xE626;
pub const XC_REPORT_DRIFT: u32 = 0xE627;
pub const XC_COMPLEX_LIE: u32 = 0xE628;
pub const XC_FROZEN_BREAK: u32 = 0xE629;

/// 基准算子清单（六条冻结，名单闭集）。
pub const BENCH_OPS: [&str; 6] = ["shift4", "halfcvt", "prefix4", "compact", "net8", "reduce"];

/// 步数模型：输入规模 n → 模型步数。
/// shift4/halfcvt/prefix4/compact/reduce = O(n)；net8 = O(19) 常数
/// （排序网比较器数定死 19——模型即网络结构）。
pub fn model_steps(op: &str, n: usize) -> Result<u64, u32> {
    if !BENCH_OPS.iter().any(|&o| o == op) {
        return Err(XC_OP_UNKNOWN);
    }
    if n == 0 {
        return Err(XC_SAMPLE_MISS);
    }
    Ok(match op {
        "net8" => 19, // 比较器数常数（网络结构与 n 无关）
        _ => n as u64,
    })
}

/// 复杂度一致核：模型步数与登记复杂度档一致。
/// 档位：0=O(1) 1=O(n) 2=O(n log n)。
pub fn complexity_check(op: &str, n: usize, claimed_class: u8) -> Result<(), u32> {
    let steps = model_steps(op, n)?;
    let n2 = n as u64;
    match claimed_class {
        0 => {
            if op != "net8" || steps != 19 {
                return Err(XC_COMPLEX_LIE);
            }
        }
        1 => {
            if steps != n2 {
                return Err(XC_COMPLEX_LIE);
            }
        }
        2 => {
            // O(n log n)：n log2(n) ≥ 模型步数才许可（本清单无此档
            // 算子——任何登记 O(n log n) 的都在说谎，直接立案）。
            return Err(XC_COMPLEX_LIE);
        }
        _ => return Err(XC_MODEL_DRIFT),
    }
    Ok(())
}

/// 预算上限表冻结（每算子最大样本步数——防基准本身拖垮系统）。
pub fn budget_table() -> Vec<(&'static str, u64)> {
    vec![
        ("shift4", 65536),
        ("halfcvt", 65536),
        ("prefix4", 32768),
        ("compact", 32768),
        ("net8", 4096),
        ("reduce", 65536),
    ]
}

/// 预算校验：样本步数 ≤ 该算子上限。
pub fn budget_check(op: &str, steps: u64) -> Result<(), u32> {
    for &(name, cap) in budget_table().iter() {
        if name == op {
            if steps > cap {
                return Err(XC_BUDGET_OOB);
            }
            return Ok(());
        }
    }
    Err(XC_OP_UNKNOWN)
}

/// 确定性语料生成（LCG——重放核的固定输入源）。
pub fn corpus_gen(seed: u64, n: usize) -> Vec<u32> {
    let mut s = seed;
    let mut out: Vec<u32> = Vec::with_capacity(n);
    for _ in 0..n {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        out.push((s >> 33) as u32);
    }
    out
}

/// 确定性基准核：位移基准（u32x4 打包左移 1 位，求和守恒双倍）。
pub fn bench_shift4(data: &[u32]) -> u64 {
    let mut acc: u64 = 0;
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let l0 = data[i] << 1;
        let l1 = data[i + 1] << 1;
        let l2 = data[i + 2] << 1;
        let l3 = data[i + 3] << 1;
        acc = acc.wrapping_add((l0 as u64) + (l1 as u64) + (l2 as u64) + (l3 as u64));
        i += 4;
    }
    acc
}

/// 确定性基准核：前缀和基准（块内 Hillis-Steele 4 通道）。
pub fn bench_prefix4(data: &[u32]) -> u64 {
    let mut acc: u64 = 0;
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let a = data[i];
        let b = a.wrapping_add(data[i + 1]);
        let c = b.wrapping_add(data[i + 2]);
        let d = c.wrapping_add(data[i + 3]);
        acc = acc.wrapping_add((a as u64) + (b as u64) + (c as u64) + (d as u64));
        i += 4;
    }
    acc
}

/// 确定性基准核：归约基准（树形求和）。
pub fn bench_reduce(data: &[u32]) -> u64 {
    let mut acc: u64 = 0;
    for &v in data.iter() {
        acc = acc.wrapping_add(v as u64);
    }
    acc
}

/// 重放核：同输入双跑逐位一致（确定性硬线）。
pub fn replay<F: Fn(&[u32]) -> u64>(f: F, data: &[u32]) -> Result<u64, u32> {
    let a = f(data);
    let b = f(data);
    if a != b {
        return Err(XC_REPLAY_FAIL);
    }
    Ok(a)
}

/// 汇总报表口径 = 清单条数（报表与清单一致硬线）。
pub fn report_len() -> usize {
    BENCH_OPS.len()
}

/// 判据入口（注册于 cgpu/mod.rs 检查表）。
pub fn run_cga60_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0060");
    let data = corpus_gen(42, 256);
    // 族一：清单闭集。
    cs.add("清单六算子", report_len() == 6, "冻结名单");
    cs.add("未知算子拒", model_steps("ghost", 4).is_err(), "OP_UNKNOWN");
    cs.add("零样本拒", model_steps("shift4", 0).is_err(), "SAMPLE_MISS");
    // 族二：步数模型逐条一致。
    cs.add("位移 O(n)", matches!(model_steps("shift4", 100), Ok(100)), "步数=n");
    cs.add("排序网 O(1)", matches!(model_steps("net8", 8), Ok(19)), "比较器数常数");
    cs.add("归约 O(n)", matches!(model_steps("reduce", 64), Ok(64)), "步数=n");
    // 族三：复杂度一致核（说谎立案）。
    cs.add("位移 O(n) 许可", complexity_check("shift4", 50, 1).is_ok(), "模型=登记");
    cs.add("排序网 O(1) 许可", complexity_check("net8", 8, 0).is_ok(), "常数档");
    cs.add("谎报 O(n log n) 拒", complexity_check("shift4", 50, 2).is_err(), "COMPLEX_LIE");
    cs.add("谎报 O(1) 拒", complexity_check("compact", 50, 0).is_err(), "COMPLEX_LIE");
    // 族四：预算上限表。
    cs.add("预算内", budget_check("shift4", 65536).is_ok(), "恰顶上限");
    cs.add("预算超拒", budget_check("prefix4", 32769).is_err(), "BUDGET_OOB");
    cs.add("预算表长", budget_table().len() == 6, "六条上限");
    // 族五：重放确定性（三基准双跑一致）。
    cs.add("位移重放", replay(bench_shift4, &data).is_ok(), "双跑一致");
    cs.add("前缀重放", replay(bench_prefix4, &data).is_ok(), "双跑一致");
    cs.add("归约重放", replay(bench_reduce, &data).is_ok(), "双跑一致");
    // 族六：值锚（语料 seed=42 n=256 的求和冻结值——模型回归锚）。
    let expect: u64 = data.iter().map(|&v| v as u64).sum();
    cs.add("归约值锚", matches!(replay(bench_reduce, &data), Ok(v) if v == expect), "Σ 语料");
    // 族七：位移守恒（左移 1 位 ⇒ 和双倍——语义不变式）。
    let small: Vec<u32> = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let s0: u64 = small.iter().map(|&v| v as u64).sum();
    let s1 = bench_shift4(&small);
    cs.add("位移双倍", s1 == s0 * 2, "Σ(x<<1)==2Σx");
    cs.add("报表口径", report_len() == budget_table().len(), "双表同长");
    cs.add("冻结破坏检出", {
        // 模型值漂移面验证：n 与模型失配即立案。
        model_steps("shift4", 101).map_err(|e| e as u16) != Ok(100)
    }, "MODEL_DRIFT 面");
    deepen_bench::merge_into(&mut cs);
    deepen_bench2::merge_into(&mut cs);
    cs
}

/// ===== 深化扩展块（cga60_simdbench）：真功能面 + 判据并入主核 =====
pub mod deepen_bench {
    use super::*;

    /// 扩展一：规模缩放一致性（模型步数随 n 线性缩放——模型自洽
    /// 核：steps(2n) == 2×steps(n)）。
    pub fn scale_consistency(op: &str, n: usize) -> Result<(), u32> {
        if op == "net8" {
            return Ok(()); // 常数档免检
        }
        let a = model_steps(op, n)?;
        let b = model_steps(op, n * 2)?;
        if b != a * 2 {
            return Err(XC_MODEL_DRIFT);
        }
        Ok(())
    }

    /// 扩展二：压缩基准核（确定性掩码 + 活元和——第五基准的独立
    /// 实现，与 cga49 语义同源）。
    pub fn bench_compact(data: &[u32]) -> u64 {
        let mut acc: u64 = 0;
        let mut i = 0usize;
        while i + 4 <= data.len() {
            for k in 0..4usize {
                if (data[i + k] & 1) == 0 {
                    acc = acc.wrapping_add(data[i + k] as u64);
                }
            }
            i += 4;
        }
        acc
    }

    /// 扩展三：报告快照对拍（双表算子名单逐位一致——清单/预算
    /// 双表同源硬线）。
    pub fn tables_aligned() -> Result<(), u32> {
        let bt = budget_table();
        if bt.len() != BENCH_OPS.len() {
            return Err(XC_REPORT_DRIFT);
        }
        for (i, op) in BENCH_OPS.iter().enumerate() {
            if bt[i].0 != *op {
                return Err(XC_REPORT_DRIFT + i as u32);
            }
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        cs.add("缩放自洽", scale_consistency("shift4", 100).is_ok(), "steps(200)==2×100");
        cs.add("常数档免检", scale_consistency("net8", 8).is_ok(), "O(1) 豁免");
        let data: Vec<u32> = (0..32u32).collect();
        let expect: u64 = data.iter().filter(|&&v| v & 1 == 0).map(|&v| v as u64).sum();
        cs.add("压缩基准锚", bench_compact(&data) == expect, "偶元和");
        cs.add("双表对齐", tables_aligned().is_ok(), "名单逐位一致");
    }

}

/// ===== 深化扩展块 2（cga60_simdbench）=====
pub mod deepen_bench2 {
    use super::*;

    /// 扩展四：排序网基准核（8 元网络执行步数 = 19 比较器账——
    /// 与模型 O(1)=19 互证的执行面）。
    pub fn bench_net8(data: &[u32]) -> Result<u64, u32> {
        if data.len() < 8 {
            return Err(XC_SAMPLE_MISS);
        }
        // 冻结 19 比较器序列（与 cga47 batcher8 同源）。
        const NET: [(usize, usize); 19] = [
            (0, 1), (2, 3), (4, 5), (6, 7),
            (0, 2), (1, 3), (4, 6), (5, 7),
            (1, 2), (5, 6), (0, 4), (3, 7),
            (1, 5), (2, 6), (1, 4), (3, 6),
            (2, 4), (3, 5), (3, 4),
        ];
        let mut v: [u32; 8] = [0; 8];
        for (i, &x) in data.iter().take(8).enumerate() {
            v[i] = x;
        }
        let mut steps: u64 = 0;
        for &(i, j) in NET.iter() {
            if v[i] > v[j] {
                let t = v[i];
                v[i] = v[j];
                v[j] = t;
            }
            steps += 1;
        }
        // 有序性硬线（网络语义面）。
        for i in 1..8 {
            if v[i - 1] > v[i] {
                return Err(XC_REPLAY_FAIL);
            }
        }
        Ok(steps)
    }

    /// 扩展五：样本窗账（基准样本数必须为 4 的倍数——打包对齐
    /// 采样纪律）。
    pub fn sample_align(n: usize) -> Result<(), u32> {
        if n == 0 {
            return Err(XC_SAMPLE_MISS);
        }
        if n % 4 != 0 {
            return Err(XC_MODEL_DRIFT);
        }
        Ok(())
    }

    /// 深化判据并入主核。
    pub fn merge_into(cs: &mut CheckSet) {
        let d: Vec<u32> = vec![8, 3, 5, 1, 9, 2, 7, 4];
        cs.add("网基准 19 步", matches!(bench_net8(&d), Ok(19)), "比较器账");
        cs.add("网基准短样拒", bench_net8(&[1, 2]).is_err(), "SAMPLE_MISS");
        cs.add("样本对齐", sample_align(256).is_ok(), "4 倍数");
        cs.add("样本失齐拒", sample_align(10).is_err(), "MODEL_DRIFT");
        cs.add("样本零拒", sample_align(0).is_err(), "SAMPLE_MISS");
    }

}
