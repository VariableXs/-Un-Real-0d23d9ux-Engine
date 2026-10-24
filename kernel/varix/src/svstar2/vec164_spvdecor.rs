//! VE-F0564 · SPIR-V 变量与装饰
//!
//! OpVariable（Function/Private/Workgroup/Uniform/StorageBuffer 五存储类——
//! 与 F0547 地址空间映射同源）、Decoration 全族（OpDecorate：Binding/
//! DescriptorSet/Location/BuiltIn/Offset/ArrayStride——接口布局的 SPIR-V 表达，
//! 与 VE 绑定模型 F0026 直映射）、成员装饰（结构体成员 Offset——std140/430
//! 布局规则在 SPIR-V 的显式表达，与 VE 布局标注一致性校验）。
//!
//! 跨批对接：上游 F0562/F0563；下游 F0565 指令族引用、F0568 反射对账。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const OP_VARIABLE: u32 = 59;
pub const OP_DECORATE: u32 = 71;
pub const OP_MEMBER_DECORATE: u32 = 72;

/// 装饰码
pub const DEC_BINDING: u32 = 33;
pub const DEC_DESCRIPTOR_SET: u32 = 34;
pub const DEC_LOCATION: u32 = 30;
pub const DEC_BUILTIN: u32 = 11;
pub const DEC_OFFSET: u32 = 35;
pub const DEC_ARRAY_STRIDE: u32 = 6;

/// 内建变量码（BuiltIn 装饰载荷）
pub const BUILTIN_POSITION: u32 = 0;
pub const BUILTIN_VERTEX_INDEX: u32 = 42;
pub const BUILTIN_FRAG_COORD: u32 = 15;
pub const BUILTIN_GLOBAL_INVOCATION_ID: u32 = 28;

/// 变量：存储类 + 结果 ID + 类型 ID
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Variable {
    pub result_id: u32,
    pub type_id: u32,
    pub storage: StorageCls,
    /// VE 侧声明来源（F0026 绑定序号，可空）
    pub ve_binding: Option<(u32, u32)>,
}

/// 存储类（五类，与 F0547 同源）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StorageCls {
    Function,
    Private,
    Workgroup,
    Uniform,
    StorageBuffer,
}

impl StorageCls {
    pub fn code(self) -> u32 {
        match self {
            StorageCls::Function => 7,
            StorageCls::Private => 6,
            StorageCls::Workgroup => 4,
            StorageCls::Uniform => 2,
            StorageCls::StorageBuffer => 12,
        }
    }
}

/// 装饰条目
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Decorate {
    pub target: u32,
    pub code: u32,
    pub payload: Vec<u32>,
}

impl Decorate {
    pub fn new(target: u32, code: u32, payload: &[u32]) -> Self {
        Decorate { target, code, payload: payload.to_vec() }
    }

    pub fn binding(target: u32, b: u32) -> Self {
        Decorate::new(target, DEC_BINDING, &[b])
    }

    pub fn descriptor_set(target: u32, ds: u32) -> Self {
        Decorate::new(target, DEC_DESCRIPTOR_SET, &[ds])
    }

    pub fn location(target: u32, loc: u32) -> Self {
        Decorate::new(target, DEC_LOCATION, &[loc])
    }

    pub fn builtin(target: u32, b: u32) -> Self {
        Decorate::new(target, DEC_BUILTIN, &[b])
    }

    pub fn offset(target: u32, off: u32) -> Self {
        Decorate::new(target, DEC_OFFSET, &[off])
    }

    pub fn array_stride(target: u32, stride: u32) -> Self {
        Decorate::new(target, DEC_ARRAY_STRIDE, &[stride])
    }
}

/// 成员装饰：结构体成员 Offset（std140/430 显式表达）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MemberDecorate {
    pub struct_type: u32,
    pub member_index: u32,
    pub code: u32,
    pub payload: Vec<u32>,
}

impl MemberDecorate {
    pub fn offset(struct_type: u32, member_index: u32, off: u32) -> Self {
        MemberDecorate { struct_type, member_index, code: DEC_OFFSET, payload: vec![off] }
    }
}

/// 装饰表：全族登记 + 接口布局一致性校验
#[derive(Clone, Debug, Default)]
pub struct DecorTable {
    pub decorates: Vec<Decorate>,
    pub members: Vec<MemberDecorate>,
    pub variables: Vec<Variable>,
}

impl DecorTable {
    pub fn new() -> Self {
        DecorTable::default()
    }

    pub fn add_var(&mut self, v: Variable) -> bool {
        if self.variables.iter().any(|x| x.result_id == v.result_id) {
            return false;
        }
        self.variables.push(v);
        true
    }

    pub fn add_decorate(&mut self, d: Decorate) {
        self.decorates.push(d);
    }

    pub fn add_member(&mut self, m: MemberDecorate) {
        self.members.push(m);
    }

    /// 绑定直映射：VE 绑定模型 F0026 (set, binding) → (DescriptorSet, Binding)
    pub fn map_ve_binding(&mut self, var_id: u32, set: u32, binding: u32) -> bool {
        if !self.variables.iter().any(|v| v.result_id == var_id) {
            return false;
        }
        self.decorates.push(Decorate::descriptor_set(var_id, set));
        self.decorates.push(Decorate::binding(var_id, binding));
        true
    }

    /// 接口布局一致性校验：同一变量重复绑定冲突检出
    pub fn binding_conflicts(&self) -> Vec<u32> {
        let mut conflicts = Vec::new();
        for i in 0..self.decorates.len() {
            for j in (i + 1)..self.decorates.len() {
                let a = &self.decorates[i];
                let b = &self.decorates[j];
                if a.target == b.target && a.code == b.code && a.payload != b.payload {
                    conflicts.push(a.target);
                }
            }
        }
        conflicts.sort();
        conflicts.dedup();
        conflicts
    }

    /// 布局一致性校验：成员 Offset 单调递增（std430 规则）
    pub fn member_offsets_monotonic(&self, struct_type: u32) -> bool {
        let offs: Vec<u32> = self
            .members
            .iter()
            .filter(|m| m.struct_type == struct_type && m.code == DEC_OFFSET)
            .map(|m| m.payload[0])
            .collect();
        offs.windows(2).all(|w| w[0] < w[1])
    }

    /// 数组 stride 一致性：stride 与元素对齐匹配（16B 规则简化）
    pub fn stride_valid(&self, target: u32, expected: u32) -> bool {
        self.decorates
            .iter()
            .find(|d| d.target == target && d.code == DEC_ARRAY_STRIDE)
            .map(|d| d.payload[0] == expected)
            .unwrap_or(false)
    }
}

pub fn run_vec164_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec164");
    // 族一：五存储类
    s.add("存储-五类编码", StorageCls::Function.code() == 7 && StorageCls::Private.code() == 6 && StorageCls::Workgroup.code() == 4, "codes");
    s.add("存储-Uniform", StorageCls::Uniform.code() == 2, "u");
    s.add("存储-StorageBuffer", StorageCls::StorageBuffer.code() == 12, "ssbo");
    // 族二：装饰全族
    let d1 = Decorate::binding(5, 0);
    s.add("装饰-Binding", d1.code == DEC_BINDING && d1.payload == vec![0], "bind");
    let d2 = Decorate::descriptor_set(5, 2);
    s.add("装饰-DescriptorSet", d2.code == DEC_DESCRIPTOR_SET && d2.payload == vec![2], "ds");
    let d3 = Decorate::location(9, 1);
    s.add("装饰-Location", d3.code == DEC_LOCATION && d3.payload == vec![1], "loc");
    let d4 = Decorate::builtin(11, BUILTIN_POSITION);
    s.add("装饰-BuiltIn", d4.code == DEC_BUILTIN && d4.payload == vec![0], "bi");
    let d5 = Decorate::offset(7, 8);
    s.add("装饰-Offset", d5.code == DEC_OFFSET && d5.payload == vec![8], "off");
    let d6 = Decorate::array_stride(7, 16);
    s.add("装饰-ArrayStride", d6.code == DEC_ARRAY_STRIDE && d6.payload == vec![16], "stride");
    // 族三：绑定直映射（F0026）
    let mut t = DecorTable::new();
    let v = Variable { result_id: 5, type_id: 3, storage: StorageCls::Uniform, ve_binding: Some((0, 0)) };
    s.add("变量-登记", t.add_var(v), "reg");
    s.add("变量-重复拒绝", !t.add_var(Variable { result_id: 5, type_id: 3, storage: StorageCls::Uniform, ve_binding: None }), "dup");
    s.add("映射-未登记变量拒绝", !t.map_ve_binding(99, 0, 0), "no-var");
    s.add("映射-直映射双装饰", t.map_ve_binding(5, 0, 1), "map");
    s.add("映射-双条目落表", t.decorates.len() == 2 && t.decorates[0].code == DEC_DESCRIPTOR_SET, "two");
    // 族四：绑定冲突检出
    t.add_decorate(Decorate::binding(5, 3));
    let cf = t.binding_conflicts();
    s.add("冲突-重复绑定检出", cf == vec![5], "conflict");
    // 族五：成员装饰与布局单调
    t.add_member(MemberDecorate::offset(7, 0, 0));
    t.add_member(MemberDecorate::offset(7, 1, 8));
    s.add("成员-单调通过", t.member_offsets_monotonic(7), "mono");
    t.add_member(MemberDecorate::offset(7, 2, 4));
    s.add("成员-乱序检出", !t.member_offsets_monotonic(7), "bad-order");
    // 族六：stride 校验
    t.add_decorate(Decorate::array_stride(7, 16));
    s.add("stride-匹配通过", t.stride_valid(7, 16), "ok");
    s.add("stride-失配拒绝", !t.stride_valid(7, 32), "bad");
    s.add("stride-未装饰拒绝", !t.stride_valid(9, 16), "none");
    // 族七：内建码
    s.add("内建-Position", BUILTIN_POSITION == 0, "pos");
    s.add("内建-VertexIndex", BUILTIN_VERTEX_INDEX == 42, "vi");
    s.add("内建-FragCoord", BUILTIN_FRAG_COORD == 15, "fc");
    s.add("内建-GlobalInvocationId", BUILTIN_GLOBAL_INVOCATION_ID == 28, "giid");
    s
}
