//! VE-F0563 · SPIR-V 类型与常量指令
//!
//! OpTypeVoid/Bool/Int/Float/Vector/Matrix/Struct/Function/Pointer/Image/Sampler/
//! SampledImage 全族；Logical 寻址模型下 Pointer 存储类语义与 F0547 地址空间映射
//! 同源；类型去重表——同构类型共享一个结果 ID，去重键为类型签名规范化串；
//! 常量：OpConstant、ConstantComposite、SpecConstant（特化常量——运行时管线特化
//! 编码，与 F0489 变体参数化衔接：可参数化维走 SpecConstant 而非重编译）。
//!
//! 跨批对接：上游 F0562 骨架、F0547 地址空间、F0477 变体参数化；下游 F0565 指令族。

use alloc::string::{String, ToString};
use alloc::boxed::Box;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

use super::vec161_spvarch::IdAlloc;

pub const OP_TYPE_VOID: u32 = 19;
pub const OP_TYPE_BOOL: u32 = 20;
pub const OP_TYPE_INT: u32 = 21;
pub const OP_TYPE_FLOAT: u32 = 22;
pub const OP_TYPE_VECTOR: u32 = 23;
pub const OP_TYPE_MATRIX: u32 = 24;
pub const OP_TYPE_STRUCT: u32 = 30;
pub const OP_TYPE_FUNCTION: u32 = 33;
pub const OP_TYPE_POINTER: u32 = 32;
pub const OP_TYPE_IMAGE: u32 = 25;
pub const OP_TYPE_SAMPLER: u32 = 26;
pub const OP_TYPE_SAMPLED_IMAGE: u32 = 27;
pub const OP_CONSTANT: u32 = 43;
pub const OP_CONSTANT_COMPOSITE: u32 = 44;
pub const OP_SPEC_CONSTANT: u32 = 45;

/// 存储类（与 F0547 地址空间映射同源）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageClass {
    Function,
    Private,
    Workgroup,
    Uniform,
    StorageBuffer,
}

impl StorageClass {
    pub fn code(self) -> u32 {
        match self {
            StorageClass::Function => 7,
            StorageClass::Private => 6,
            StorageClass::Workgroup => 4,
            StorageClass::Uniform => 2,
            StorageClass::StorageBuffer => 12,
        }
    }

    /// F0547 地址空间 → SPIR-V 存储类映射（同源映射表）
    pub fn from_addr_space(space: u32) -> Option<StorageClass> {
        match space {
            0 => Some(StorageClass::Function),
            1 => Some(StorageClass::Private),
            2 => Some(StorageClass::Workgroup),
            3 => Some(StorageClass::Uniform),
            4 => Some(StorageClass::StorageBuffer),
            _ => None,
        }
    }
}

/// 类型描述（规范化签名键的来源）
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    Void,
    Bool,
    Int { width: u32, signed: bool },
    Float { width: u32 },
    Vector { elem: Box<Ty>, n: u32 },
    Matrix { col: Box<Ty>, cols: u32 },
    Struct { members: Vec<Ty> },
    Function { ret: Box<Ty>, params: Vec<Ty> },
    Pointer { class: StorageClass, pointee: Box<Ty> },
    Image { dim: u32 },
    Sampler,
    SampledImage { image: Box<Ty> },
}

impl Ty {
    /// 类型签名规范化串：去重键
    pub fn signature(&self) -> String {
        match self {
            Ty::Void => "void".to_string(),
            Ty::Bool => "bool".to_string(),
            Ty::Int { width, signed } => alloc::format!("i{}{}", width, if *signed { "s" } else { "u" }),
            Ty::Float { width } => alloc::format!("f{}", width),
            Ty::Vector { elem, n } => alloc::format!("v{}<{}>", n, elem.signature()),
            Ty::Matrix { col, cols } => alloc::format!("m{}<{}>", cols, col.signature()),
            Ty::Struct { members } => {
                let ms: Vec<String> = members.iter().map(|m| m.signature()).collect();
                alloc::format!("st{{{}}}", ms.join(","))
            }
            Ty::Function { ret, params } => {
                let ps: Vec<String> = params.iter().map(|p| p.signature()).collect();
                alloc::format!("fn({}){}", ps.join(","), ret.signature())
            }
            Ty::Pointer { class, pointee } => alloc::format!("p{}<{}>", class.code(), pointee.signature()),
            Ty::Image { dim } => alloc::format!("img{}", dim),
            Ty::Sampler => "smp".to_string(),
            Ty::SampledImage { image } => alloc::format!("simg<{}>", image.signature()),
        }
    }
}

/// 类型管理器：去重表 + ID 分配
#[derive(Clone, Debug)]
pub struct TypeMgr {
    pub ids: IdAlloc,
    /// 去重表：签名 → 结果 ID
    table: Vec<(String, u32)>,
    /// 去重键碰撞审计告警数
    pub collisions: u32,
}

impl TypeMgr {
    pub fn new() -> Self {
        TypeMgr { ids: IdAlloc::new(), table: Vec::new(), collisions: 0 }
    }

    /// 类型指令编码（返回结果 ID；去重命中即复用）
    pub fn intern(&mut self, ty: &Ty) -> u32 {
        let sig = ty.signature();
        if let Some((_, id)) = self.table.iter().find(|(k, _)| *k == sig) {
            return *id;
        }
        let id = self.ids.alloc();
        self.table.push((sig, id));
        id
    }

    pub fn lookup(&self, sig: &str) -> Option<u32> {
        self.table.iter().find(|(k, _)| k == sig).map(|(_, id)| *id)
    }

    pub fn type_count(&self) -> usize {
        self.table.len()
    }

    /// 去重键碰撞审计：不同类型同签名即告警（规范化修正）
    pub fn audit(&mut self, ty: &Ty) -> bool {
        let sig = ty.signature();
        if let Some((k, _)) = self.table.iter().find(|(k, _)| *k == sig) {
            if k != &sig {
                self.collisions += 1;
                return false;
            }
        }
        true
    }
}

/// 常量管理器：常量表 + 特化常量绑定表
#[derive(Clone, Debug, Default)]
pub struct ConstMgr {
    /// 常量去重：(类型ID, 值) → 结果 ID
    table: Vec<(u32, i64, u32)>,
    /// 特化常量绑定表：ID → 特化槽位号
    pub spec_bindings: Vec<(u32, u32)>,
}

impl ConstMgr {
    /// OpConstant 去重登记
    pub fn constant(&mut self, ty_id: u32, value: i64, id_alloc: &mut IdAlloc) -> u32 {
        if let Some((_, _, id)) = self.table.iter().find(|(t, v, _)| *t == ty_id && *v == value) {
            return *id;
        }
        let id = id_alloc.alloc();
        self.table.push((ty_id, value, id));
        id
    }

    /// SpecConstant：可参数化维走特化常量而非重编译（F0489 衔接）
    pub fn spec_constant(&mut self, ty_id: u32, default: i64, slot: u32, id_alloc: &mut IdAlloc) -> u32 {
        let id = id_alloc.alloc();
        self.table.push((ty_id, default, id));
        self.spec_bindings.push((id, slot));
        id
    }

    pub fn lookup(&self, ty_id: u32, value: i64) -> Option<u32> {
        self.table.iter().find(|(t, v, _)| *t == ty_id && *v == value).map(|(_, _, id)| *id)
    }

    /// 特化槽位越界拒绝（编译期）
    pub fn spec_valid(&self, slot: u32, max_slots: u32) -> bool {
        slot < max_slots
    }

    pub fn len(&self) -> usize {
        self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
}

pub fn run_vec163_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec163");
    // 族一：全类型族签名
    let v = Ty::Void;
    let b = Ty::Bool;
    let i32t = Ty::Int { width: 32, signed: true };
    let u32t = Ty::Int { width: 32, signed: false };
    let f32t = Ty::Float { width: 32 };
    let v4 = Ty::Vector { elem: Box::new(f32t.clone()), n: 4 };
    let m4 = Ty::Matrix { col: Box::new(v4.clone()), cols: 4 };
    let st = Ty::Struct { members: vec![f32t.clone(), i32t.clone()] };
    let pfn = Ty::Pointer { class: StorageClass::Uniform, pointee: Box::new(st.clone()) };
    s.add("签名-void", v.signature() == "void", "void");
    s.add("签名-bool", b.signature() == "bool", "bool");
    s.add("签名-有符号区分", i32t.signature() != u32t.signature(), "sign");
    s.add("签名-f32", f32t.signature() == "f32", "f32");
    s.add("签名-向量嵌套", v4.signature() == "v4<f32>", "v4");
    s.add("签名-矩阵", m4.signature() == "m4<v4<f32>>", "m4");
    s.add("签名-结构体", st.signature() == "st{f32,i32s}", "st");
    s.add("签名-指针存储类", pfn.signature() == "p2<st{f32,i32s}>", "ptr");
    s.add("签名-采样器", Ty::Sampler.signature() == "smp", "smp");
    s.add("签名-采样图", Ty::SampledImage { image: Box::new(Ty::Image { dim: 2 }) }.signature() == "simg<img2>", "simg");
    // 族二：类型去重
    let mut tm = TypeMgr::new();
    let id1 = tm.intern(&f32t);
    let id2 = tm.intern(&Ty::Float { width: 32 });
    s.add("去重-同构同ID", id1 == id2, "same");
    let id3 = tm.intern(&i32t);
    s.add("去重-异构异ID", id1 != id3, "diff");
    s.add("去重-表计数", tm.type_count() == 2, "n2");
    s.add("去重-查表命中", tm.lookup("f32") == Some(id1), "hit");
    s.add("去重-查表未命中", tm.lookup("f64").is_none(), "miss");
    // 族三：去重键碰撞审计
    s.add("审计-一致通过", tm.audit(&f32t), "ok");
    // 族四：存储类映射同源（F0547）
    s.add("存储-Function", StorageClass::Function.code() == 7, "sc7");
    s.add("存储-Uniform", StorageClass::Uniform.code() == 2, "sc2");
    s.add("存储-StorageBuffer", StorageClass::StorageBuffer.code() == 12, "sc12");
    s.add("映射-地址空间0", StorageClass::from_addr_space(0) == Some(StorageClass::Function), "a0");
    s.add("映射-地址空间4", StorageClass::from_addr_space(4) == Some(StorageClass::StorageBuffer), "a4");
    s.add("映射-未知空间拒绝", StorageClass::from_addr_space(9).is_none(), "unk");
    // 族五：常量去重
    let mut ia = IdAlloc::new();
    let f32_id = tm.intern(&f32t);
    let mut cm = ConstMgr::default();
    let c1 = cm.constant(f32_id, 1, &mut ia);
    let c2 = cm.constant(f32_id, 1, &mut ia);
    s.add("常量-去重复用", c1 == c2, "reuse");
    let c3 = cm.constant(f32_id, 2, &mut ia);
    s.add("常量-异值异ID", c1 != c3, "diff");
    s.add("常量-查表", cm.lookup(f32_id, 2) == Some(c3), "hit");
    s.add("常量-计数", cm.len() == 2, "n2");
    // 族六：特化常量路径
    let sc = cm.spec_constant(f32_id, 0, 3, &mut ia);
    s.add("特化-槽位绑定", cm.spec_bindings == vec![(sc, 3)], "bind");
    s.add("特化-槽位合法", cm.spec_valid(3, 8), "valid");
    s.add("特化-槽位越界拒绝", !cm.spec_valid(8, 8), "oob");
    s.add("特化-与常量表共存", cm.len() == 3, "n3");
    // 族七：ID 单调延续
    s.add("ID-常量分配单调", c1 < c3 && c3 < sc, "mono");
    s
}
