//! VE-F0565 · SPIR-V 指令族生成
//!
//! IR 指令 → SPIR-V 指令：算术逻辑（OpIAdd/OpFAdd/OpFMul 全族——SPIR-V 无
//! 运算符重载，每运算一指令）、向量矩阵构造（OpCompositeConstruct/Extract/
//! Insert——向量 swizzle 在 SPIR-V 是 CompositeExtract+Construct 组合）、
//! 采样指令（OpImageSampleImplicitLod/ExplicitLod/Dref 族——F0471 标注映射）、
//! 原子（OpAtomicIAdd 等）。
//!
//! 跨批对接：上游 F0562/F0563/F0564；下游 F0566 控制流、F0569 对拍。

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;

pub const OP_IADD: u32 = 128;
pub const OP_ISUB: u32 = 129;
pub const OP_IMUL: u32 = 130;
pub const OP_FADD: u32 = 129 - 9; // 120
pub const OP_FSUB: u32 = 131;
pub const OP_FMUL: u32 = 133;
pub const OP_FDIV: u32 = 135;
pub const OP_SLT: u32 = 170;
pub const OP_COMPOSITE_CONSTRUCT: u32 = 80;
pub const OP_COMPOSITE_EXTRACT: u32 = 81;
pub const OP_COMPOSITE_INSERT: u32 = 82;
pub const OP_IMAGE_SAMPLE_IMPLICIT_LOD: u32 = 87;
pub const OP_IMAGE_SAMPLE_EXPLICIT_LOD: u32 = 88;
pub const OP_IMAGE_SAMPLE_DREF_IMPLICIT_LOD: u32 = 89;
pub const OP_ATOMIC_IADD: u32 = 238;
pub const OP_ATOMIC_LOAD: u32 = 227;
pub const OP_ATOMIC_STORE: u32 = 228;
pub const OP_LOAD: u32 = 61;
pub const OP_STORE: u32 = 62;

/// 单条 SPIR-V 指令（逻辑层）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Inst {
    pub opcode: u32,
    pub result_id: Option<u32>,
    pub result_type: Option<u32>,
    pub operands: Vec<u32>,
}

impl Inst {
    pub fn pure(opcode: u32, operands: &[u32]) -> Self {
        Inst { opcode, result_id: None, result_type: None, operands: operands.to_vec() }
    }

    pub fn with_result(opcode: u32, result_type: u32, result_id: u32, operands: &[u32]) -> Self {
        Inst { opcode, result_id: Some(result_id), result_type: Some(result_type), operands: operands.to_vec() }
    }
}

/// IR 二元运算 → SPIR-V 算术逻辑指令映射（每运算一指令，无重载）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    IAdd,
    ISub,
    IMul,
    FAdd,
    FSub,
    FMul,
    FDiv,
    SLt,
}

impl BinOp {
    pub fn opcode(self) -> u32 {
        match self {
            BinOp::IAdd => OP_IADD,
            BinOp::ISub => OP_ISUB,
            BinOp::IMul => OP_IMUL,
            BinOp::FAdd => OP_FADD,
            BinOp::FSub => OP_FSUB,
            BinOp::FMul => OP_FMUL,
            BinOp::FDiv => OP_FDIV,
            BinOp::SLt => OP_SLT,
        }
    }

    /// 按类型宽度选择整型/浮点变体（类型驱动映射）
    pub fn for_type(self, is_float: bool) -> u32 {
        match (self, is_float) {
            (BinOp::IAdd, false) | (BinOp::FAdd, true) => self.opcode(),
            (BinOp::ISub, false) | (BinOp::FSub, true) => self.opcode(),
            (BinOp::IMul, false) | (BinOp::FMul, true) => self.opcode(),
            (BinOp::FDiv, true) => self.opcode(),
            (BinOp::IAdd, true) => OP_FADD,
            (BinOp::FAdd, false) => OP_IADD,
            (BinOp::ISub, true) => OP_FSUB,
            (BinOp::FSub, false) => OP_ISUB,
            (BinOp::IMul, true) => OP_FMUL,
            (BinOp::FMul, false) => OP_IMUL,
            (BinOp::FDiv, false) => OP_FDIV,
            (BinOp::SLt, _) => OP_SLT,
        }
    }
}

/// 指令生成器：ID 由外部分配器预分配传入（保持单调）
#[derive(Clone, Debug, Default)]
pub struct InstGen {
    pub insts: Vec<Inst>,
}

impl InstGen {
    /// 算术逻辑：result = op a, b
    pub fn binop(&mut self, op: BinOp, is_float: bool, ty: u32, res: u32, a: u32, b: u32) -> u32 {
        self.insts.push(Inst::with_result(op.for_type(is_float), ty, res, &[a, b]));
        res
    }

    /// swizzle 组合：vec4.xyz → CompositeExtract + CompositeConstruct
    /// indices 为分量下标序列
    pub fn swizzle(&mut self, src: u32, ty: u32, res_base: u32, indices: &[u32]) -> u32 {
        let mut extracted: Vec<u32> = Vec::new();
        let mut cur = res_base;
        for (k, i) in indices.iter().enumerate() {
            cur += 1;
            self.insts.push(Inst::with_result(OP_COMPOSITE_EXTRACT, ty, cur, &[src, *i]));
            extracted.push(cur);
        }
        self.insts.push(Inst::with_result(OP_COMPOSITE_CONSTRUCT, ty, cur + 1, &extracted));
        cur + 1
    }

    /// 采样指令映射（F0471 标注驱动）
    pub fn sample(&mut self, lod_mode: LodMode, dref: bool, ty: u32, res: u32, img: u32, coord: u32, lod: Option<u32>) -> u32 {
        let opcode = match (lod_mode, dref) {
            (LodMode::Implicit, false) => OP_IMAGE_SAMPLE_IMPLICIT_LOD,
            (LodMode::Explicit, false) => OP_IMAGE_SAMPLE_EXPLICIT_LOD,
            (LodMode::Implicit, true) => OP_IMAGE_SAMPLE_DREF_IMPLICIT_LOD,
            (LodMode::Explicit, true) => OP_IMAGE_SAMPLE_DREF_IMPLICIT_LOD + 1,
        };
        let mut ops = vec![img, coord];
        if let Some(l) = lod {
            ops.push(l);
        }
        self.insts.push(Inst::with_result(opcode, ty, res, &ops));
        res
    }

    /// 原子指令
    pub fn atomic_add(&mut self, ty: u32, res: u32, ptr: u32, scope: u32, sem: u32, val: u32) -> u32 {
        self.insts.push(Inst::with_result(OP_ATOMIC_IADD, ty, res, &[ptr, scope, sem, val]));
        res
    }

    /// 存取
    pub fn load(&mut self, ty: u32, res: u32, ptr: u32) {
        self.insts.push(Inst::with_result(OP_LOAD, ty, res, &[ptr]));
    }

    pub fn store(&mut self, ptr: u32, val: u32) {
        self.insts.push(Inst::pure(OP_STORE, &[ptr, val]));
    }
}

/// LOD 模式（F0471 标注）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LodMode {
    Implicit,
    Explicit,
}

/// 指令序稳定校验：同输入生成的指令序列逐条相等
pub fn stable(insts: &[Inst], expected_opcodes: &[u32]) -> bool {
    insts.len() == expected_opcodes.len() && insts.iter().zip(expected_opcodes.iter()).all(|(i, o)| i.opcode == *o)
}

pub fn run_vec165_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec165");
    // 族一：算术逻辑映射
    s.add("算术-IAdd", BinOp::IAdd.opcode() == OP_IADD, "iadd");
    s.add("算术-FAdd", BinOp::FAdd.opcode() == OP_FADD, "fadd");
    s.add("算术-每运算一指令", BinOp::IAdd.opcode() != BinOp::FAdd.opcode(), "distinct");
    s.add("算术-浮点变体", BinOp::IAdd.for_type(true) == OP_FADD, "float");
    s.add("算术-整型变体", BinOp::FAdd.for_type(false) == OP_IADD, "int");
    s.add("算术-比较映射", BinOp::SLt.for_type(false) == OP_SLT, "slt");
    // 族二：生成器产出
    let mut g = InstGen::default();
    let r1 = g.binop(BinOp::FAdd, true, 2, 10, 4, 5);
    s.add("生成-结果ID透传", r1 == 10, "id");
    s.add("生成-操作数序", g.insts[0].operands == vec![4, 5], "ops");
    s.add("生成-带类型与结果", g.insts[0].result_type == Some(2) && g.insts[0].result_id == Some(10), "meta");
    // 族三：swizzle 组合
    let mut g2 = InstGen::default();
    let r = g2.swizzle(3, 2, 100, &[0, 2, 1]);
    s.add("swizzle-两段构造", r == 104, "id");
    s.add("swizzle-先Extract后Construct", stable(&g2.insts, &[OP_COMPOSITE_EXTRACT, OP_COMPOSITE_EXTRACT, OP_COMPOSITE_EXTRACT, OP_COMPOSITE_CONSTRUCT]), "order");
    s.add("swizzle-分量下标", g2.insts[0].operands == vec![3, 0] && g2.insts[2].operands == vec![3, 1], "idx");
    s.add("swizzle-Construct汇入", g2.insts[3].operands == vec![101, 103, 102], "merge");
    // 族四：采样族映射（F0471）
    let mut g3 = InstGen::default();
    g3.sample(LodMode::Implicit, false, 2, 20, 4, 5, None);
    g3.sample(LodMode::Explicit, false, 2, 21, 4, 5, Some(9));
    g3.sample(LodMode::Implicit, true, 2, 22, 4, 5, None);
    s.add("采样-ImplicitLod", g3.insts[0].opcode == OP_IMAGE_SAMPLE_IMPLICIT_LOD, "impl");
    s.add("采样-ExplicitLod带LOD", g3.insts[1].opcode == OP_IMAGE_SAMPLE_EXPLICIT_LOD && g3.insts[1].operands == vec![4, 5, 9], "expl");
    s.add("采样-Dref族", g3.insts[2].opcode == OP_IMAGE_SAMPLE_DREF_IMPLICIT_LOD, "dref");
    // 族五：原子族
    let mut g4 = InstGen::default();
    g4.atomic_add(2, 30, 7, 1, 2, 9);
    s.add("原子-AtomicIAdd", g4.insts[0].opcode == OP_ATOMIC_IADD, "ai");
    s.add("原子-四操作数", g4.insts[0].operands == vec![7, 1, 2, 9], "ops4");
    // 族六：存取
    let mut g5 = InstGen::default();
    g5.load(2, 40, 7);
    g5.store(7, 40);
    s.add("存取-Load", g5.insts[0].opcode == OP_LOAD && g5.insts[0].operands == vec![7], "load");
    s.add("存取-Store", g5.insts[1].opcode == OP_STORE && g5.insts[1].operands == vec![7, 40], "store");
    s.add("存取-Store无结果", g5.insts[1].result_id.is_none(), "void");
    s
}
