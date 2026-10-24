//! cga105_codecbench —— CGPU-F0105 编解码算子性能入 Bench。
//! 锚点（CGPU 施工书）：CGPU-F0105 · 编解码算子性能入 Bench · 目标 300 行
//! 判据摘录：Bench 项闭集（每编解码算子一条：ID/后端/像素数/
//! 步数模型/实测步——五行结构）；步数模型对拍（实测 ≤ 模型×1.5
//! ——白盒上界核，模型源自 cga90 总表同构）；入册幂等（同 (ID,
//! 后端) 重入册拒——行锁）；离群拒（实测/模型 > 3 → 拒收立案
//! ——异常面）；冻结名单（入册后名单指纹冻结——改册即变）；
//! 域闸（像素数 0 / 后端越界拒）。
//! 组件：BenchRow + BenchBook + 入册（行锁+离群拒）+ 模型对拍 +
//!   汇总账（总步数/占比）+ 指纹。
//! 职责定位：编解码域的性能总账——cga90（通用算子表）的编解码
//! 分册；白盒步数模型先行（预期账），实测是核预期不是自由测
//! 量（防性能回归静默漂移）。
//!
//! 步数模型（编解码分册冻结式）：
//!   解码 IDCT：steps = blocks × 256（8×8 双 1D）
//!   反滤波：steps = rows × width（每字节一次预测）
//!   量化：steps = blocks × 64
//!   上采：steps = out_px × 4（双线性四采样）

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE9E0 段 · 本模块独占）=====
pub const XC_DUP_ROW: u32 = 0xE9E0; // 行锁
pub const XC_N_ZERO: u32 = 0xE9E1; // 像素/块数为 0
pub const XC_BACKEND_OOB: u32 = 0xE9E2; // 后端越界
pub const XC_OUTLIER: u32 = 0xE9E3; // 离群（实测/模型 > 3）
pub const XC_OVER_BUDGET: u32 = 0xE9E4; // 实测超模型上界 ×1.5

// ===== 后端闭集（复用 cga104 语义）=====
pub const B_SIMD: u8 = 0;
pub const B_HW: u8 = 1;

/// Bench 行。
#[derive(Clone)]
pub struct BenchRow {
    pub op_id: u16,
    pub backend: u8,
    pub n_units: u32, // 块/行/像素数（模型入参）
    pub model_steps: u64,
    pub measured_steps: u64,
}

/// 步数模型（op 族闭集）。
/// op 族：0=IDCT 1=defilter 2=quant 3=upsample。
pub fn step_model(family: u8, n: u32) -> Result<u64, u32> {
    if n == 0 {
        return Err(XC_N_ZERO);
    }
    match family {
        0 => Ok(n as u64 * 256),
        1 => Ok(n as u64), // n = 总字节数
        2 => Ok(n as u64 * 64),
        3 => Ok(n as u64 * 4),
        _ => Err(XC_BACKEND_OOB), // 未知族（借用码）
    }
}

/// Bench 册。
pub struct BenchBook {
    pub rows: Vec<BenchRow>,
    pub frame_budget: u64,
}

impl BenchBook {
    pub fn new(frame_budget: u64) -> BenchBook {
        BenchBook { rows: Vec::new(), frame_budget }
    }

    /// 入册（行锁 + 离群拒 + 上界核）。
    pub fn enroll(&mut self, op_id: u16, family: u8, backend: u8, n: u32, measured: u64) -> Result<(), u32> {
        if backend > B_HW {
            return Err(XC_BACKEND_OOB);
        }
        if self.rows.iter().any(|r| r.op_id == op_id && r.backend == backend) {
            return Err(XC_DUP_ROW);
        }
        let model_steps = step_model(family, n)?;
        // 上界核：实测 ≤ 模型×1.5。
        if measured > model_steps + model_steps / 2 {
            return Err(XC_OVER_BUDGET);
        }
        // 离群立案：实测/模型 > 3 不入册（结构异常面——虽被上界
        // 覆盖，双闸保显性）。
        if measured > model_steps * 3 {
            return Err(XC_OUTLIER);
        }
        self.rows.push(BenchRow { op_id, backend, n_units: n, model_steps, measured_steps: measured });
        Ok(())
    }

    pub fn find(&self, op_id: u16, backend: u8) -> Option<&BenchRow> {
        self.rows.iter().find(|r| r.op_id == op_id && r.backend == backend)
    }

    /// 总实测步数。
    pub fn total_measured(&self) -> u64 {
        self.rows.iter().map(|r| r.measured_steps).sum()
    }

    /// 帧占比（千分比）。
    pub fn permille(&self) -> u64 {
        if self.frame_budget == 0 {
            return 0;
        }
        self.total_measured() * 1000 / self.frame_budget
    }

    /// 名单指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for r in self.rows.iter() {
            for v in [r.op_id as u64, r.backend as u64, r.n_units as u64, r.model_steps, r.measured_steps].iter() {
                h ^= *v;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
        h
    }
}

// ===== 主判据 =====
pub fn run_cga105_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0105");
    // 族一：模型锚。
    cs.add("IDCT 锚", step_model(0, 10) == Ok(2560), "10 块×256");
    cs.add("defilter 锚", step_model(1, 1000) == Ok(1000), "字节一步");
    cs.add("quant 锚", step_model(2, 10) == Ok(640), "块×64");
    cs.add("upsample 锚", step_model(3, 100) == Ok(400), "×4");
    cs.add("零块拒", step_model(0, 0).is_err(), "N=0");
    cs.add("未知族拒", step_model(9, 10).is_err(), "闭集外");
    // 族二：入册（行锁 + 上界）。
    let mut b = BenchBook::new(1_000_000);
    cs.add("入册 IDCT", b.enroll(9301, 0, B_SIMD, 64, 15_000).is_ok(), "模型 16384 内");
    cs.add("行锁", b.enroll(9301, 0, B_SIMD, 64, 15_000).is_err(), "重入册拒");
    cs.add("异后端可入", b.enroll(9301, 0, B_HW, 64, 5_000).is_ok(), "(ID,后端) 键");
    cs.add("入册 defilter", b.enroll(9401, 1, B_SIMD, 4096, 4_200).is_ok(), "模型 4096×1.5");
    // 族三：上界核（实测超 ×1.5 拒）。
    cs.add("上界拒", b.enroll(9501, 2, B_SIMD, 10, 2000).is_err(), "640×1.5=960 < 2000");
    cs.add("离群拒", b.enroll(9502, 3, B_SIMD, 10, 500).is_err(), "40×3=120 < 500");
    // 族四：后端域闸。
    cs.add("后端越界拒", b.enroll(9601, 0, 9, 10, 10).is_err(), "BACKEND_OOB");
    // 族五：汇总账。
    // 总实测 = 15000 + 5000 + 4200 = 24200。
    cs.add("总实测锚", b.total_measured() == 24_200, "三行合计");
    cs.add("占比锚", b.permille() == 24, "24200×1000/1e6");
    // 族六：指纹。
    let f1 = b.fingerprint();
    cs.add("指纹幂等", b.fingerprint() == f1, "双算一致");
    let _ = b.enroll(9701, 2, B_SIMD, 10, 500);
    cs.add("指纹敏感", b.fingerprint() != f1, "加行变指");
    // 族七：查行。
    cs.add("查行命中", b.find(9301, B_SIMD).map(|r| r.measured_steps) == Some(15_000), "键查");
    cs.add("查行未中", b.find(9301, 9).is_none(), "越界后端查空");
    CheckSet::merge(CheckSet::merge(cs, run_cga105_deep()), run_cga105_deep2())
}

// ===== 深化面：入册往返与总账 =====
/// 入册→查找往返：measured 落账一致。
pub fn enroll_roundtrip(budget: u64) -> bool {
    let mut b = BenchBook::new(budget);
    if b.enroll(1, 0, B_SIMD, 64, 500).is_err() {
        return false;
    }
    match b.find(1, B_SIMD) {
        Some(row) => row.measured_steps == 500,
        None => false,
    }
}

pub fn run_cga105_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga105_deep");
    cs.add("入册往返", enroll_roundtrip(100000), "measured 落账");
    let mut b = BenchBook::new(100000);
    let _ = b.enroll(1, 0, B_SIMD, 64, 500);
    cs.add("行锁", b.enroll(1, 0, B_SIMD, 64, 500).is_err(), "DUP_ROW");
    cs.add("n=0 拒", b.enroll(2, 0, B_SIMD, 0, 500).is_err(), "N_ZERO");
    cs.add("后端越界拒", b.enroll(2, 0, 9, 64, 500).is_err(), "BACKEND_OOB");
    cs.add("缺行 None", b.find(9, 9).is_none(), "不在册");
    cs.add("总实测正", b.total_measured() == 500, "累计一致");
    cs.add("千分比闭", b.permille() <= 1000, "≤1000");
    cs.add("指纹稳定", b.fingerprint() == b.fingerprint(), "双算同值");
    cs.add("模型越界拒", step_model(200, 64).is_err(), "family 闭");
    cs
}

// ===== 深化面二：离群闸与多行账 =====
pub fn run_cga105_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga105_deep2");
    let mut b = BenchBook::new(1_000_000);
    // 模型内入册。
    let m1 = step_model(0, 64).unwrap_or(1000);
    cs.add("入册收", b.enroll(1, 0, B_SIMD, 64, m1).is_ok(), "模型内");
    // 离群：实测 = 模型 ×10 → 拒（OUTLIER）。
    cs.add("离群拒", b.enroll(2, 0, B_SIMD, 64, m1 * 10).is_err(), "OUTLIER");
    // n=0 拒双验。
    cs.add("n0 拒双验", b.enroll(3, 0, B_SIMD, 0, m1).is_err(), "幂等");
    // 多行入册与查找。
    let m2 = step_model(1, 128).unwrap_or(2000);
    let _ = b.enroll(4, 1, B_HW, 128, m2);
    cs.add("双行查找", b.find(1, B_SIMD).is_some() && b.find(4, B_HW).is_some(), "行齐");
    cs.add("行独立", b.find(4, B_SIMD).is_none(), "未录组合 None");
    // 总实测 = Σ。
    cs.add("总实测合计", b.total_measured() == m1 + m2, "Σ行");
    cs.add("千分界", b.permille() <= 1000, "≤1000");
    // 指纹随行变。
    let fp0 = b.fingerprint();
    let m3 = step_model(0, 32).unwrap_or(500);
    let _ = b.enroll(5, 0, B_HW, 32, m3);
    cs.add("指纹敏感", b.fingerprint() != fp0, "行变指纹变");
    // 模型闭：family 越界拒。
    cs.add("模型闭", step_model(9, 64).is_err(), "族越界");
    cs.add("后端闭", b.enroll(6, 0, 7, 32, 10).is_err(), "BACKEND_OOB");
    cs.add("预算界", BenchBook::new(1000).permille() <= 1000, "空书千分");
    cs
}
