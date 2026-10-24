//! VE-F0562 · SPIR-V 模块生成
//!
//! SPIR-V 模块骨架：Header（魔数/版本/生成器编号/边界 ID 上限/Schema）、
//! OpCapability 声明（按 IR 用到的特性最小声明）、OpExtInstImport（GLSL.std.450
//! 扩展指令集导入——内建函数的载体）、OpMemoryModel（Vulkan 语义）、
//! OpEntryPoint 与接口列表生成。ID 分配器：SPIR-V 一切皆 ID（结果 ID 单调分配
//! 策略——确定性 F0494 的 SPIR-V 版）。
//!
//! 跨批对接：上游 F0561 架构；下游 F0563/F0564/F0565 引用骨架。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vec161_spvarch::{Capability, IdAlloc, WordStream};

/// OpCodes（本册所需子集）
pub const OP_CAPABILITY: u32 = 17;
pub const OP_EXT_INST_IMPORT: u32 = 11;
pub const OP_MEMORY_MODEL: u32 = 14;
pub const OP_ENTRY_POINT: u32 = 15;
pub const OP_NAME: u32 = 5;

/// Vulkan 内存模型与 Logical 寻址模型编码
pub const MEMORY_MODEL_VULKAN: u32 = 3;
pub const ADDRESSING_LOGICAL: u32 = 2;

/// 入口阶段
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Vertex,
    Fragment,
    Compute,
}

impl Stage {
    pub fn exec_model(self) -> u32 {
        match self {
            Stage::Vertex => 0,
            Stage::Fragment => 4,
            Stage::Compute => 5,
        }
    }
}

/// 字符串编码为 SPIR-V Literal String（NUL 结尾补齐 4 字节）
pub fn literal_string(text: &str) -> Vec<u32> {
    let bytes: Vec<u8> = text.bytes().chain(core::iter::once(0)).collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut w: u32 = 0;
        for j in 0..4 {
            if i + j < bytes.len() {
                w |= (bytes[i + j] as u32) << (8 * j);
            }
        }
        words.push(w);
        i += 4;
    }
    words
}

/// 模块骨架构建器
#[derive(Clone, Debug)]
pub struct ModuleBuilder {
    pub stream: WordStream,
    pub ids: IdAlloc,
    /// GLSL.std.450 导入的结果 ID
    pub glsl450_id: Option<u32>,
    /// 已声明能力（去重）
    caps: Vec<Capability>,
}

impl ModuleBuilder {
    pub fn new(version: u32) -> Self {
        let mut stream = WordStream::new();
        // 边界 ID 上限先占位，装配完修正
        stream.header(version, 1);
        ModuleBuilder { stream, ids: IdAlloc::new(), glsl450_id: None, caps: Vec::new() }
    }

    /// OpCapability：最小化去重声明
    pub fn declare_capability(&mut self, cap: Capability) -> bool {
        if self.caps.contains(&cap) {
            return false;
        }
        self.caps.push(cap);
        self.stream.instruction(OP_CAPABILITY, &[cap.code()]);
        true
    }

    /// OpExtInstImport "GLSL.std.450"
    pub fn import_glsl450(&mut self) -> u32 {
        let id = self.ids.alloc();
        let lit = literal_string("GLSL.std.450");
        let mut ops = vec![id];
        ops.extend(lit);
        self.stream.instruction(OP_EXT_INST_IMPORT, &ops);
        self.glsl450_id = Some(id);
        id
    }

    /// OpMemoryModel Logical Vulkan
    pub fn memory_model(&mut self) {
        self.stream.instruction(OP_MEMORY_MODEL, &[ADDRESSING_LOGICAL, MEMORY_MODEL_VULKAN]);
    }

    /// OpEntryPoint：执行模型 + 入口 ID + 名称 + 接口列表
    pub fn entry_point(&mut self, stage: Stage, entry_id: u32, name: &str, interface: &[u32]) -> u32 {
        let lit = literal_string(name);
        let mut ops = vec![stage.exec_model(), entry_id];
        ops.extend(lit);
        ops.extend_from_slice(interface);
        self.stream.instruction(OP_ENTRY_POINT, &ops);
        entry_id
    }

    /// OpName（调试命名）
    pub fn name(&mut self, target: u32, name: &str) {
        let lit = literal_string(name);
        let mut ops = vec![target];
        ops.extend(lit);
        self.stream.instruction(OP_NAME, &ops);
    }

    /// 收口：按实际 ID 数修正头中边界 ID
    pub fn finalize(mut self) -> WordStream {
        self.stream.words[3] = self.ids.bound;
        self.stream
    }

    pub fn has_capability(&self, cap: Capability) -> bool {
        self.caps.contains(&cap)
    }
}

/// Capability 最小化判定：IR 特性 → 所需能力集
pub fn caps_for_ir(uses_image_gather: bool, uses_frag_depth: bool) -> Vec<Capability> {
    let mut caps = vec![Capability::Shader];
    if uses_image_gather {
        caps.push(Capability::ImageGatherExtended);
    }
    let _ = uses_frag_depth;
    caps
}

pub fn run_vec162_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec162");
    // 族一：Literal String 编码
    let l1 = literal_string("main");
    s.add("串-四字对齐", l1.len() == 2, "pad");
    s.add("串-NUL结尾", l1[1] & 0xFF == 0, "nul");
    let l2 = literal_string("");
    s.add("串-空串一字", l2.len() == 1 && l2[0] == 0, "empty");
    let l3 = literal_string("GLSL.std.450");
    s.add("串-扩展名长度", l3.len() == 3, "glsl");
    // 族二：骨架五段
    let mut b = ModuleBuilder::new(0x0001_0300);
    s.add("骨架-头五字", b.stream.header_ok(), "hdr");
    s.add("能力-Shader最小", b.declare_capability(Capability::Shader), "cap");
    let glsl = b.import_glsl450();
    s.add("扩展-导入ID分配", glsl == 1, "ext");
    s.add("扩展-登记在案", b.glsl450_id == Some(1), "reg");
    b.memory_model();
    let entry = b.ids.alloc();
    let var1 = b.ids.alloc();
    let ep = b.entry_point(Stage::Vertex, entry, "vs_main", &[var1]);
    s.add("入口-执行模型", b.stream.words.contains(&(ep)) && Stage::Vertex.exec_model() == 0, "ep");
    s.add("入口-ID不变", ep == entry, "id");
    // 族三：Capability 去重
    s.add("能力-重复拒绝", !b.declare_capability(Capability::Shader), "dup");
    s.add("能力-去重登记", b.has_capability(Capability::Shader), "has");
    // 族四：ID 单调（与 F0561 同源）
    let next = b.ids.next();
    s.add("ID-继续单调", next == 3, "next3");
    b.name(var1, "pos");
    let fin = b.finalize();
    s.add("收口-边界ID修正", fin.words[3] == 3, "bound");
    s.add("收口-魔数保持", fin.words[0] == 0x0723_0203, "magic");
    // 族五：执行模型编码
    s.add("阶段-Vertex", Stage::Vertex.exec_model() == 0, "v");
    s.add("阶段-Fragment", Stage::Fragment.exec_model() == 4, "f");
    s.add("阶段-Compute", Stage::Compute.exec_model() == 5, "c");
    // 族六：Capability 最小化判定
    let c0 = caps_for_ir(false, false);
    s.add("判定-纯Shader", c0.len() == 1, "min0");
    let c1 = caps_for_ir(true, false);
    s.add("判定-采集追加", c1.len() == 2, "min1");
    // 族七：内存模型 Vulkan 语义
    let mut b2 = ModuleBuilder::new(0x0001_0300);
    b2.memory_model();
    let w = b2.stream.words;
    let idx = w.len() - 2;
    s.add("模型-Logical寻址", w[idx] == 2, "logical");
    s.add("模型-Vulkan内存", w[idx + 1] == 3, "vulkan");
    s
}
