//! cga45_simdshift —— CGPU-F0045 SIMD 位移与掩码工具库。
//! 锚点（CGPU 施工书）：CGPU-F0045 · SIMD 位移与掩码工具库 · 目标 340 行
//! 判据摘录：打包标量通道独立移位语义正确（算术/逻辑两族）；掩码
//!   寄存器 merge/zero 两模式混合结果与 AVX-512 语义逐位一致；
//!   移位量 ≥ 位宽必须钳制不 UB；掩码位图闭集（k0~k7 八寄存器）。
//! 组件：打包标量模型（4×u32 / 2×u64 两通道宽）+ 通道移位引擎
//!   （逻辑/算术 × 左/右 四族，越界钳制到位宽-1 或清零按模式）+
//!   掩码寄存器堆（8×u64，k0 恒全 1 硬线）+ 掩码混合引擎（merge
//!   模式保留目标位、zero 模式清零非掩码位——逐位与 AVX-512 对拍）
//!   + 桶形移位器参考实现（对拍锚）。
//! 职责定位：CPU SIMD 路径的位级地基——位移与掩码是所有 SIMD
//!   算子的公共子表达式，语义必须与硬件对拍一致；本模块把
//!   「打包标量在软件里是什么」钉死为可测的整数模型。
//! 性能逐项分解：通道移位 O(通道数)=O(4)；掩码混合 O(位宽/64)；
//!   对拍核 O(混合位数)。全整数确定性。
//! 诊断码段：0xE500（0xE501~0xE509 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const SS_VERSION: &str = "cga45_simdshift-v1";

/// 诊断段基址（0xE500 段 · 本模块独占）。
pub const SS_SEG_BASE: u16 = 0xE500;

/// XcCode 闭集（0xE501~0xE509）。
pub const XC_SHIFT_OOB: u32 = 0xE501;
pub const XC_MASK_IDX_OOR: u32 = 0xE502;
pub const XC_K0_IMMUTABLE: u32 = 0xE503;
pub const XC_MERGE_DRIFT: u32 = 0xE504;
pub const XC_LANE_OOR: u32 = 0xE505;
pub const XC_ARITH_SIGN: u32 = 0xE506;
pub const XC_BARREL_DRIFT: u32 = 0xE507;
pub const XC_MODE_OOR: u32 = 0xE508;
pub const XC_WIDTH_UNSUP: u32 = 0xE509;

/// 通道宽枚举：4×u32 与 2×u64 两族（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneWidth {
    W4xU32,
    W2xU64,
}

/// 混合模式（AVX-512 语义闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlendMode {
    /// merge：掩码位 1 取源，0 保留目标。
    Merge,
    /// zero：掩码位 1 取源，0 清零。
    Zero,
}

impl BlendMode {
    pub fn code(self) -> u8 {
        match self {
            BlendMode::Merge => 0,
            BlendMode::Zero => 1,
        }
    }
}

/// 4×u32 打包标量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackU32 {
    pub lanes: [u32; 4],
}

impl PackU32 {
    pub fn new(lanes: [u32; 4]) -> PackU32 {
        PackU32 { lanes }
    }

    /// 通道逻辑左移：移位量逐通道独立；≥32 视为全零通道（IEEE
    /// 移位语义：无 UB，越界即出界清零）。
    pub fn shl_each(&self, amounts: [u32; 4]) -> PackU32 {
        let mut out = [0u32; 4];
        for i in 0..4 {
            out[i] = if amounts[i] >= 32 { 0 } else { self.lanes[i] << amounts[i] };
        }
        PackU32 { lanes: out }
    }

    /// 通道逻辑右移（越界清零）。
    pub fn lshr_each(&self, amounts: [u32; 4]) -> PackU32 {
        let mut out = [0u32; 4];
        for i in 0..4 {
            out[i] = if amounts[i] >= 32 { 0 } else { self.lanes[i] >> amounts[i] };
        }
        PackU32 { lanes: out }
    }

    /// 通道算术右移：符号位填充（越界 = 全符号扩展）。
    pub fn ashr_each(&self, amounts: [u32; 4]) -> PackU32 {
        let mut out = [0u32; 4];
        for i in 0..4 {
            let v = self.lanes[i] as i32;
            out[i] = if amounts[i] >= 31 { if v < 0 { u32::MAX } else { 0 } } else { ((v >> amounts[i]) as u32) & 0xFFFF_FFFF };
        }
        PackU32 { lanes: out }
    }

    /// 桶形移位参考实现：32 步逐位扫描（对拍锚，O(32) 换正确性）。
    pub fn barrel_shl_ref(&self, amount: u32) -> PackU32 {
        let mut out = [0u32; 4];
        for i in 0..4 {
            let mut acc: u32 = 0;
            for b in 0..32u32 {
                if amount <= b {
                    let src_bit = b - amount;
                    acc |= ((self.lanes[i] >> src_bit) & 1) << b;
                }
            }
            out[i] = acc;
        }
        PackU32 { lanes: out }
    }
}

/// 2×u64 打包标量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackU64 {
    pub lanes: [u64; 2],
}

impl PackU64 {
    pub fn new(lanes: [u64; 2]) -> PackU64 {
        PackU64 { lanes }
    }

    /// 通道逻辑左移（≥64 清零）。
    pub fn shl_each(&self, amounts: [u32; 2]) -> PackU64 {
        let mut out = [0u64; 2];
        for i in 0..2 {
            out[i] = if amounts[i] >= 64 { 0 } else { self.lanes[i] << amounts[i] };
        }
        PackU64 { lanes: out }
    }

    /// 通道逻辑右移（≥64 清零）。
    pub fn lshr_each(&self, amounts: [u32; 2]) -> PackU64 {
        let mut out = [0u64; 2];
        for i in 0..2 {
            out[i] = if amounts[i] >= 64 { 0 } else { self.lanes[i] >> amounts[i] };
        }
        PackU64 { lanes: out }
    }
}

/// 掩码寄存器堆：k0~k7 八个 64 位掩码，k0 硬线恒全 1。
pub struct MaskRegs {
    regs: [u64; 8],
}

impl MaskRegs {
    /// 新建：k0 恒全 1（硬件不变式）。
    pub fn new() -> MaskRegs {
        let mut r = [0u64; 8];
        r[0] = u64::MAX;
        MaskRegs { regs: r }
    }

    /// 写 k1..k7；写 k0 拒收（硬线）。
    pub fn write(&mut self, idx: usize, val: u64) -> Result<(), u16> {
        if idx >= 8 {
            return Err(XC_MASK_IDX_OOR as u16);
        }
        if idx == 0 && val != u64::MAX {
            return Err(XC_K0_IMMUTABLE as u16);
        }
        self.regs[idx] = val;
        Ok(())
    }

    /// 读。
    pub fn read(&self, idx: usize) -> Result<u64, u16> {
        if idx >= 8 {
            return Err(XC_MASK_IDX_OOR as u16);
        }
        Ok(self.regs[idx])
    }

    /// k0 不变式核。
    pub fn k0_all_ones(&self) -> bool {
        self.regs[0] == u64::MAX
    }
}

/// 掩码混合引擎：按掩码位逐通道混合 src/dst（AVX-512 语义）。
pub fn blend_u32(mask: u64, src: &PackU32, dst: &PackU32, mode: BlendMode) -> Result<PackU32, u16> {
    // 4 通道打包标量只消费掩码低 4 位（每通道 1 位——dword 语义）。
    if (mask & !0xFu64) != 0 {
        return Err(XC_MERGE_DRIFT as u16);
    }
    let mut out = [0u32; 4];
    for i in 0..4 {
        let bit = (mask >> i) & 1;
        out[i] = match mode {
            BlendMode::Merge => {
                if bit == 1 { src.lanes[i] } else { dst.lanes[i] }
            }
            BlendMode::Zero => {
                if bit == 1 { src.lanes[i] } else { 0 }
            }
        };
    }
    Ok(PackU32 { lanes: out })
}

/// 域自检入口：移位/掩码/混合/对拍 四族。
pub fn run_cga45_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0045-SIMD位移掩码");
    // 族一：通道移位语义。
    let p = PackU32::new([0x8000_0001, 0x0000_00F0, 0xFFFF_0000, 1]);
    let s = p.shl_each([1, 4, 0, 32]);
    cs.add("左移逐通道", s.lanes[0] == 0x0000_0002 && s.lanes[1] == 0x0000_0F00
        && s.lanes[2] == 0xFFFF_0000 && s.lanes[3] == 0, "越界通道清零");
    let r = p.lshr_each([31, 4, 32, 1]);
    cs.add("逻辑右移", r.lanes[0] == 1 && r.lanes[1] == 0xF && r.lanes[2] == 0 && r.lanes[3] == 0,
           "越界清零红线");
    let a = p.ashr_each([31, 0, 0, 0]);
    cs.add("算术右移符号", a.lanes[0] == u32::MAX && a.lanes[1] == 0x0000_00F0, "负数全符号扩展");

    // 族二：桶形移位对拍（参考实现 vs 原生移位，逐通道逐位一致）。
    let b = p.barrel_shl_ref(3);
    let n = p.shl_each([3, 3, 3, 3]);
    cs.add("桶形对拍一致", b == n, "逐位参考 vs 原生");

    // 族三：掩码寄存器堆（k0 硬线 + 越界拒）。
    let mut mr = MaskRegs::new();
    cs.add("k0 恒全1", mr.k0_all_ones(), "硬件不变式");
    cs.add("写 k0 拒", mr.write(0, 0).is_err(), "k0 硬线");
    cs.add("写 k5 生效", mr.write(5, 0b1010).is_ok() && mr.read(5) == Ok(0b1010), "k1..k7 可写");
    cs.add("越界读写拒", mr.write(8, 1).is_err() && mr.read(9).is_err(), "闭集外拒收");

    // 族四：掩码混合（merge/zero 双模式对拍）。
    let src = PackU32::new([100, 200, 300, 400]);
    let dst = PackU32::new([1, 2, 3, 4]);
    let m = blend_u32(0b0101, &src, &dst, BlendMode::Merge);
    cs.add("merge 语义", matches!(&m, Ok(v) if v.lanes == [100, 2, 300, 4]), "1 取源 0 留目标");
    let z = blend_u32(0b0101, &src, &dst, BlendMode::Zero);
    cs.add("zero 语义", matches!(&z, Ok(v) if v.lanes == [100, 0, 300, 0]), "1 取源 0 清零");
    cs.add("掩码高位拒", blend_u32(0x10, &src, &dst, BlendMode::Merge).is_err(), "4 通道只吃低 4 位");

    // u64 通道。
    let p64 = PackU64::new([0x8000_0000_0000_0001, 0xF0]);
    let s64 = p64.shl_each([1, 64]);
    cs.add("u64 左移", s64.lanes[0] == 0x0000_0000_0000_0002 && s64.lanes[1] == 0, "≥64 清零");
    cs
}

// ===== 扩展面：双字混合（32 位粒度掩码）+ 逐位对拍 + 掩码密度账 =====
/// 8 通道 u16 打包标量（word 粒度：掩码低 8 位每通道 1 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackU16 {
    pub lanes: [u16; 8],
}

impl PackU16 {
    pub fn new(lanes: [u16; 8]) -> PackU16 {
        PackU16 { lanes }
    }

    /// 通道移位（≥16 清零）。
    pub fn shl_each(&self, amounts: [u32; 8]) -> PackU16 {
        let mut out = [0u16; 8];
        for i in 0..8 {
            out[i] = if amounts[i] >= 16 { 0 } else { self.lanes[i] << amounts[i] };
        }
        PackU16 { lanes: out }
    }

    /// word 粒度掩码混合（掩码只吃低 8 位）。
    pub fn blend(mask: u64, src: &PackU16, dst: &PackU16, mode: BlendMode) -> Result<PackU16, u16> {
        if (mask & !0xFFu64) != 0 {
            return Err(XC_MERGE_DRIFT as u16);
        }
        let mut out = [0u16; 8];
        for i in 0..8 {
            let bit = (mask >> i) & 1;
            out[i] = match mode {
                BlendMode::Merge => if bit == 1 { src.lanes[i] } else { dst.lanes[i] },
                BlendMode::Zero => if bit == 1 { src.lanes[i] } else { 0 },
            };
        }
        Ok(PackU16 { lanes: out })
    }
}

/// 掩码密度账：掩码字里 1 的千分比（SIMD 活跃度画像——低活跃
/// 通道可提示切换标量路径省电）。
pub fn mask_density_permille(mask: u64) -> u32 {
    (mask.count_ones() * 1000 / 64) as u32
}

/// 混合对拍：blend(Merge) 与手写逐通道展开结果一致（语义双实现
/// 互证——一张账两条通道，漂移即显性）。
pub fn blend_ref(mask: u64, src: &PackU32, dst: &PackU32) -> Result<PackU32, u16> {
    if (mask & !0xFu64) != 0 {
        return Err(XC_MERGE_DRIFT as u16);
    }
    let mut out = [0u32; 4];
    for i in 0..4 {
        out[i] = if (mask >> i) & 1 == 1 { src.lanes[i] } else { dst.lanes[i] };
    }
    Ok(PackU32 { lanes: out })
}

/// 桶形右移参考（与 barrel_shl_ref 对偶——右移逐位扫描锚）。
pub fn barrel_shr_ref(p: &PackU32, amount: u32) -> PackU32 {
    let mut out = [0u32; 4];
    for i in 0..4 {
        let mut acc: u32 = 0;
        for b in 0..32u32 {
            let src_bit = b + amount;
            if src_bit < 32 {
                acc |= ((p.lanes[i] >> src_bit) & 1) << b;
            }
        }
        out[i] = acc;
    }
    PackU32 { lanes: out }
}

/// 移位预算账：SIMD 通道移位吞吐步数账（每通道移位 1 步，4 通道
/// 打包一次 4 步；预算红线外拒收——防上层把移位当万能胶水）。
pub const SHIFT_BUDGET_STEPS: u64 = 4096;

pub struct ShiftBudget {
    pub used: u64,
}

impl ShiftBudget {
    pub fn new() -> ShiftBudget {
        ShiftBudget { used: 0 }
    }

    /// 记一次打包移位（4 步）；超预算拒收。
    pub fn charge_pack(&mut self) -> Result<(), u16> {
        if self.used + 4 > SHIFT_BUDGET_STEPS {
            return Err(XC_SHIFT_OOB as u16);
        }
        self.used += 4;
        Ok(())
    }

    /// 记一次 u64 打包移位（2 步）。
    pub fn charge_pack64(&mut self) -> Result<(), u16> {
        if self.used + 2 > SHIFT_BUDGET_STEPS {
            return Err(XC_SHIFT_OOB as u16);
        }
        self.used += 2;
        Ok(())
    }

    pub fn remaining(&self) -> u64 {
        SHIFT_BUDGET_STEPS - self.used
    }
}
