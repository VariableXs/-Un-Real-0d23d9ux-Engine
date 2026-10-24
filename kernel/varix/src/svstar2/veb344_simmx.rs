//! veb344_simmx —— VE-F0344 SIMD 指令派发完整矩阵。
//! 锚点：VE-F0344 · 目标 360 行。
//! 判据原文：五维矩阵（SSE4 / AVX2 / AVX-512 / NEON / RVV）、
//!           双保险（运行时探测 × 编译期路径）、显性回落
//!           （未覆盖组合回落标量并登记补齐清单）、每路径
//!           冒烟（失败禁用回落并告警）、判据。
//! 组件：派发矩阵 + 探测记录 + 补齐清单。
//! 职责定位：派发矩阵是「五语翻译台」——同一个内核五种方言
//!           都能念；没翻译的方言就老老实实念标量原文并登记
//!           「这句还没翻」，冒烟不过的翻译当场撕掉禁用。
//! 性能逐项分解：探测一次性；派发 O(1) 查表；冒烟 O(路径数)
//!           启动期。
//! 诊断码段：0x31F00（0x31F01~0x31F09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SM_VERSION: &str = "veb344_simmx-v1";

/// 诊断段基址（0x31F00 段 · 本模块独占）。
pub const SM_SEG_BASE: u32 = 0x31F00;
pub const XC_BADISA: u32 = 0x31F01; // 指令集非法
pub const XC_BADKER: u32 = 0x31F02; // 内核号非法
pub const XC_SMOKE: u32 = 0x31F03; // 冒烟失败（路径禁用）
pub const XC_MISMATCH: u32 = 0x31F04; // 探测与编译期不一致

/// 五维指令集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isa {
    Sse4,
    Avx2,
    Avx512,
    Neon,
    Rvv,
}

/// 指集序号。
pub fn isa_no(i: Isa) -> u32 {
    match i {
        Isa::Sse4 => 0,
        Isa::Avx2 => 1,
        Isa::Avx512 => 2,
        Isa::Neon => 3,
        Isa::Rvv => 4,
    }
}

/// 向量宽（字节数，派发依据之一）。
pub fn isa_width(i: Isa) -> u32 {
    match i {
        Isa::Sse4 => 16,
        Isa::Avx2 => 32,
        Isa::Avx512 => 64,
        Isa::Neon => 16,
        Isa::Rvv => 0, // 可变长
    }
}

/// 派发路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// SIMD 路径。
    Simd,
    /// 标量回落。
    Scalar,
    /// 已禁用（冒烟失败）。
    Banned,
}

/// 内核号界（0~15 共十六个软渲内核）。
pub const KER_MAX: u32 = 16;

/// 派发矩阵。
pub struct DispMx {
    /// [内核][指集] → 路径。
    m: [[Path; 5]; KER_MAX as usize],
    /// 探测在案指集位图。
    probed: u32,
    /// 编译期在案指集位图。
    compiled: u32,
    /// 补齐清单（回落组合）。
    pub todo: Vec<(u32, u32)>,
    /// 告警数。
    pub alerts: u32,
}

impl DispMx {
    /// 建：初始全标量、全登记。
    pub fn new(compiled_mask: u32) -> DispMx {
        let mut m = [[Path::Scalar; 5]; KER_MAX as usize];
        let mut todo = Vec::new();
        for k in 0..KER_MAX as usize {
            for i in 0..5 {
                todo.push((k as u32, i as u32));
            }
        }
        DispMx {
            m,
            probed: 0,
            compiled: compiled_mask,
            todo,
            alerts: 0,
        }
    }

    /// 运行时探测登记（一次性；重复幂等）。
    pub fn probe(&mut self, i: Isa) {
        self.probed |= 1 << isa_no(i);
        self.refresh(i);
    }

    /// 刷新指集列：探测∩编译期 → Simd 并从补齐清单摘除。
    fn refresh(&mut self, i: Isa) {
        let no = isa_no(i) as usize;
        let both = self.probed & self.compiled & (1 << no);
        for k in 0..KER_MAX as usize {
            if both != 0 && self.m[k][no] != Path::Banned {
                self.m[k][no] = Path::Simd;
                self.todo.retain(|x| !(x.0 == k as u32 && x.1 == no as u32));
            }
        }
    }

    /// 双保险核：探测与编译期不一致 → 保守交集（返回交集掩码）。
    pub fn safe_set(&self) -> Result<u32, u32> {
        let both = self.probed & self.compiled;
        let either = self.probed | self.compiled;
        if both != either {
            return Err(XC_MISMATCH);
        }
        Ok(both)
    }

    /// 派发：内核×指集查表。
    pub fn route(&self, ker: u32, i: Isa) -> Result<Path, u32> {
        if ker >= KER_MAX {
            return Err(XC_BADKER);
        }
        Ok(self.m[ker as usize][isa_no(i) as usize])
    }

    /// 冒烟：失败禁该指集全列并告警。
    pub fn smoke(&mut self, i: Isa, ok: bool) -> Result<(), u32> {
        if ok {
            return Ok(());
        }
        let no = isa_no(i) as usize;
        for k in 0..KER_MAX as usize {
            self.m[k][no] = Path::Banned;
        }
        self.alerts += 1;
        Err(XC_SMOKE)
    }

    /// 补齐清单长。
    pub fn todo_len(&self) -> usize {
        self.todo.len()
    }

    /// 路径计数（Simd/Scalar/Banned）。
    pub fn tally(&self) -> (u32, u32, u32) {
        let mut c = (0u32, 0u32, 0u32);
        for row in self.m.iter() {
            for p in row.iter() {
                match p {
                    Path::Simd => c.0 += 1,
                    Path::Scalar => c.1 += 1,
                    Path::Banned => c.2 += 1,
                }
            }
        }
        c
    }
}

pub fn run_veb344_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0344");
    // 族零：双保险。
    let mut m = DispMx::new(0b00011); // 编译期：SSE4+AVX2
    s.add("未探测交集零", m.safe_set() == Ok(0), "空交集");
    m.probe(Isa::Sse4);
    s.add("双保险过", m.safe_set() == Ok(0b00001), "SSE4 双全");
    s.add("派发 SIMD", m.route(0, Isa::Sse4) == Ok(Path::Simd), "查表命中");
    s.add("派发标量", m.route(0, Isa::Neon) == Ok(Path::Scalar), "未探测回落");
    // 族一：五维派发。
    m.probe(Isa::Avx2);
    s.add("AVX2 命中", m.route(3, Isa::Avx2) == Ok(Path::Simd), "宽 32");
    s.add("宽序核", isa_width(Isa::Avx512) == 64, "512 位");
    s.add("RVV 变长", isa_width(Isa::Rvv) == 0, "可变宽");
    s.add("指集五", isa_no(Isa::Rvv) == 4, "第五维");
    // 族二：显性回落与补齐清单。
    s.add("回落在册", m.todo_len() > 0, "未覆盖组合登记");
    let _ = m.route(0, Isa::Neon);
    // 探测 NEON（编译期无）→ 保守交集不扩 → 列仍标量。
    m.probe(Isa::Neon);
    s.add("单保险不派", m.route(0, Isa::Neon) == Ok(Path::Scalar), "编译期缺席");
    s.add("不一致显性", m.safe_set() == Err(XC_MISMATCH), "探测≠编译");
    // 族三：冒烟。
    let mut m2 = DispMx::new(0b11111);
    for i in [Isa::Sse4, Isa::Avx2, Isa::Avx512, Isa::Neon, Isa::Rvv] {
        m2.probe(i);
    }
    s.add("全维派发", m2.route(7, Isa::Rvv) == Ok(Path::Simd), "五维齐");
    s.add("冒烟过", m2.smoke(Isa::Avx512, true).is_ok(), "健康");
    s.add("冒烟败禁用", m2.smoke(Isa::Avx512, false) == Err(XC_SMOKE), "撕翻译");
    s.add("禁用回落", m2.route(7, Isa::Avx512) == Ok(Path::Banned), "全列禁");
    s.add("告警一", m2.alerts == 1, "告警在案");
    let (simd, scalar, banned) = m2.tally();
    s.add("计四维活", simd == 64 && banned == 16, "80 格");
    s.add("标量零", scalar == 0, "全探测");
    // 族四：内核号门。
    s.add("内核越界拒", m2.route(16, Isa::Sse4) == Err(XC_BADKER), "≥16");
    s.add("顶号过", m2.route(15, Isa::Sse4) == Ok(Path::Simd), "15 号");
    // 族五：初始全标量。
    let m3 = DispMx::new(0);
    let t3 = m3.tally();
    s.add("初始全标量", t3.1 == 80, "80 格");
    s.add("补齐八十", m3.todo_len() == 80, "全登记");
    s.add("魔数核", SM_SEG_BASE == 0x31F00, "段基址");
    s
}
