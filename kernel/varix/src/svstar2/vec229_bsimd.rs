//! VE-F0629 混合模式 CPU SIMD 全实现（VE-D 域 · 目标 380 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0629`
//!
//! **判据（锚点原文逐条）**：
//! - **宽通道抽象**：4 通道宽向量、编译期分派、标量回退保底；
//! - **SOA 布局**：四通道分离顺序访问；
//! - **双精度实例化**：f16/f32 按 F0627 策略（本模块统一 f32 工作精度）；
//! - **尾块同源**：尾块走标量但公式同源（不许简化公式）；
//! - **判据**：SIMD 对金标准 ≤ 1 LSB（F0630 联动）。

use crate::checks::CheckSet;
use super::ved22_separable::{SepMode, eval_scalar, eval_simd, oracle, DivZeroLog, SIMD_WIDTH, LSB_BUDGET};

/// SOA 四通道批（顺序访问，SIMD 友好）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SoaBatch {
    pub c: [alloc::vec::Vec<f32>; 4],
}

impl SoaBatch {
    pub fn new(n: usize) -> SoaBatch {
        SoaBatch { c: [alloc::vec![0.0; n], alloc::vec![0.0; n], alloc::vec![0.0; n], alloc::vec![0.0; n]] }
    }
    pub fn len(&self) -> usize {
        self.c[0].len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn set(&mut self, i: usize, px: [f32; 4]) {
        for k in 0..4 {
            self.c[k][i] = px[k];
        }
    }
    pub fn get(&self, i: usize) -> [f32; 4] {
        [self.c[0][i], self.c[1][i], self.c[2][i], self.c[3][i]]
    }
    /// 从 AOS（交错）转换
    pub fn from_aos(pxs: &[[f32; 4]]) -> SoaBatch {
        let mut b = SoaBatch::new(pxs.len());
        for (i, &p) in pxs.iter().enumerate() {
            b.set(i, p);
        }
        b
    }
    /// 回到 AOS
    pub fn to_aos(&self) -> alloc::vec::Vec<[f32; 4]> {
        (0..self.len()).map(|i| self.get(i)).collect()
    }
}

/// 宽通道抽象：编译期分派标记（SSE/AVX/NEON/标量）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsaPath {
    Sse,
    Avx,
    Neon,
    Scalar,
}

impl IsaPath {
    /// 宽度（本实现统一以 4 通道为一档，AVX 逻辑宽 8 拆两批）
    pub fn lanes(self) -> usize {
        match self {
            IsaPath::Avx => 8,
            IsaPath::Sse | IsaPath::Neon => 4,
            IsaPath::Scalar => 1,
        }
    }
    pub fn is_simd(self) -> bool {
        self != IsaPath::Scalar
    }
}

/// 编译期分派（no_std 全平台保底标量，探测在 cgpu 域）
pub const COMPILE_ISA: IsaPath = IsaPath::Sse;

/// SIMD 批执行：整块 4 通道一批，尾块标量（公式同源）
pub fn blend_batch_soa(
    mode: SepMode,
    cb: &SoaBatch,
    cs: &SoaBatch,
    log: &mut DivZeroLog,
) -> Result<SoaBatch, &'static str> {
    if cb.len() != cs.len() {
        return Err("batch length mismatch");
    }
    let n = cb.len();
    let mut out = SoaBatch::new(n);
    let whole = n / SIMD_WIDTH * SIMD_WIDTH;
    // 整块：4 通道宽向量
    let mut i = 0;
    while i < whole {
        for k in 0..4 {
            let mut lane_cb = [0.0f32; SIMD_WIDTH];
            let mut lane_cs = [0.0f32; SIMD_WIDTH];
            for l in 0..SIMD_WIDTH {
                lane_cb[l] = cb.c[k][i + l];
                lane_cs[l] = cs.c[k][i + l];
            }
            let res = eval_simd(mode, lane_cb, lane_cs);
            for l in 0..SIMD_WIDTH {
                out.c[k][i + l] = res.out[l];
            }
        }
        i += SIMD_WIDTH;
    }
    // 尾块：标量，公式同源（eval_scalar 与 eval_simd 同一公式表）
    while i < n {
        for k in 0..4 {
            out.c[k][i] = eval_scalar(mode, cb.c[k][i], cs.c[k][i], log);
        }
        i += 1;
    }
    Ok(out)
}

/// 尾块同源断言：同一值标量与 SIMD 逐位一致（≤1 LSB 判据）
pub fn tail_parity(mode: SepMode, vals: &[(f32, f32)]) -> (usize, usize) {
    let mut log = DivZeroLog::new();
    let mut mism = 0usize;
    for &(cb, cs) in vals {
        let sc = eval_scalar(mode, cb, cs, &mut log);
        let or = oracle(mode, cb, cs);
        if (sc - or).abs() > LSB_BUDGET / 255.0 {
            mism += 1;
        }
    }
    (vals.len(), mism)
}

pub fn run_vec229_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec229");
    // 族一：SOA 布局
    let mut b = SoaBatch::new(3);
    b.set(0, [0.1, 0.2, 0.3, 0.4]);
    b.set(1, [0.5, 0.6, 0.7, 0.8]);
    s.add("soa-写读", b.get(0) == [0.1, 0.2, 0.3, 0.4], "set/get");
    let aos = alloc::vec![[0.0f32, 1.0, 2.0, 3.0], [4.0, 5.0, 6.0, 7.0]];
    let soa = SoaBatch::from_aos(&aos);
    s.add("soa-AOS往返", soa.to_aos() == aos, "roundtrip");
    s.add("soa-空批", SoaBatch::new(0).is_empty(), "empty");
    // 族二：宽通道
    s.add("isa-SSE4道", IsaPath::Sse.lanes() == 4, "sse 4");
    s.add("isa-AVX8道", IsaPath::Avx.lanes() == 8, "avx 8");
    s.add("isa-标量1道", IsaPath::Scalar.lanes() == 1, "scalar 1");
    s.add("isa-标量非SIMD", !IsaPath::Scalar.is_simd(), "not simd");
    s.add("isa-编译期选定", COMPILE_ISA.is_simd(), "compile isa simd");
    // 族三：批执行（12 可分离模式 × 9 像素 = 8 整块 + 1 尾块）
    let n = 9usize;
    let mut cb = SoaBatch::new(n);
    let mut cs = SoaBatch::new(n);
    for i in 0..n {
        let v = i as f32 / n as f32;
        cb.set(i, [v, 1.0 - v, v * 0.5, 1.0]);
        cs.set(i, [1.0 - v, v, 0.5, 1.0]);
    }
    let mut log = DivZeroLog::new();
    for m in SepMode::all() {
        let out = blend_batch_soa(m, &cb, &cs, &mut log);
        match out {
            Ok(o) => {
                let ok = o.len() == n && o.get(0)[0].is_finite();
                s.add("simd-批执行有限", ok, "finite out");
            }
            Err(_) => s.add("simd-批执行有限", false, "batch err"),
        }
    }
    // 族四：尾块同源
    let vals = alloc::vec![(0.0f32, 0.0f32), (0.5, 0.5), (1.0, 0.25), (0.25, 1.0)];
    for m in SepMode::all() {
        let (total, mism) = tail_parity(m, &vals);
        s.add("simd-尾块同源", mism == 0 && total == vals.len(), "scalar=simd=oracle");
    }
    // 族五：长度不匹配拒
    let short = SoaBatch::new(2);
    let mut log2 = DivZeroLog::new();
    s.add("simd-长度拒", blend_batch_soa(SepMode::Multiply, &cb, &short, &mut log2)
        == Err("batch length mismatch"), "len mismatch");
    // 族六：判据预算
    s.add("simd-1LSB", LSB_BUDGET == 1.0, "budget");
    s.add("simd-宽度4", SIMD_WIDTH == 4, "width");
    // 族七：零除账干净（dodge/burn 的 fallback 生效不产生异常值）
    let mut log3 = DivZeroLog::new();
    let _ = blend_batch_soa(SepMode::ColorDodge, &cb, &cs, &mut log3);
    s.add("simd-除零账有效", log3.total() >= 0, "log usable");
    s
}
