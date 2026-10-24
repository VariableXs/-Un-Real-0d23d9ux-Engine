//! VE-F0561 · SPIR-V 后端架构
//!
//! 与前三个文本后端的本质差异——直接产出二进制模块（Word 流），无文本中间态；
//! 架构五段：Word 流装配器、ID 分配器（单调分配）、指令编码器、验证通路
//! （spirv-val 为硬门禁）、产物封装；Vulkan 语义目标声明（Shader 能力集最小化）；
//! 确定性继承（F0494）：Word 序列确定性生成——ID 单调、指令序稳定、无环境相关分支；
//! 绿色底线：首批产物 spirv-val 全量通过。
//!
//! 跨批对接：上游 F0479 流水线产物；下游 F0567 验证对接、F0578 总对拍第四路。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// SPIR-V 魔数（小端序 0x07230203）
pub const MAGIC: u32 = 0x0723_0203;
/// Schema 固定为 0
pub const SCHEMA: u32 = 0;
/// 生成器编号（VE 注册值占位）
pub const GENERATOR: u32 = 0x0056_0001;
/// ID 边界上限
pub const ID_BOUND_MAX: u32 = 0x4000_0000;

/// Word 流装配器：Header + 指令 Word 序列
#[derive(Clone, Debug, Default)]
pub struct WordStream {
    pub words: Vec<u32>,
}

impl WordStream {
    pub fn new() -> Self {
        WordStream { words: Vec::new() }
    }

    /// Header 五字：魔数/版本/生成器/边界 ID 上限/Schema
    pub fn header(&mut self, version: u32, id_bound: u32) {
        self.words.clear();
        self.words.push(MAGIC);
        self.words.push(version);
        self.words.push(GENERATOR);
        self.words.push(id_bound);
        self.words.push(SCHEMA);
    }

    /// 指令编码：word0 = (word_count << 16) | opcode
    pub fn instruction(&mut self, opcode: u32, operands: &[u32]) {
        let word_count = 1 + operands.len() as u32;
        self.words.push((word_count << 16) | opcode);
        self.words.extend_from_slice(operands);
    }

    pub fn len_words(&self) -> usize {
        self.words.len()
    }

    /// 合法性预检：头五字齐备且魔数正确
    pub fn header_ok(&self) -> bool {
        self.words.len() >= 5
            && self.words[0] == MAGIC
            && self.words[4] == SCHEMA
    }
}

/// ID 分配器：结果 ID 单调分配（确定性 F0494 的 SPIR-V 版）
#[derive(Clone, Debug)]
pub struct IdAlloc {
    next_id: u32,
    /// 边界（模块头写入的 ID 上限）
    pub bound: u32,
}

impl IdAlloc {
    pub fn new() -> Self {
        IdAlloc { next_id: 1, bound: 1 }
    }

    /// 单调分配：永不复用、永不回退
    pub fn alloc(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        if self.next_id > self.bound {
            self.bound = self.next_id;
        }
        id
    }

    pub fn next(&self) -> u32 {
        self.next_id
    }

    /// ID 耗尽边界告警
    pub fn near_exhaustion(&self) -> bool {
        self.next_id >= ID_BOUND_MAX - 1
    }
}

/// 能力集声明：Shader 能力集最小化
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    Shader,
    /// 按需逐项判定才启用
    ImageGatherExtended,
}

impl Capability {
    pub fn code(self) -> u32 {
        match self {
            Capability::Shader => 1,
            Capability::ImageGatherExtended => 25,
        }
    }
}

/// 产物封装：模块 + 未验标记（fail-closed：未验模块禁止流出）
#[derive(Clone, Debug)]
pub struct Module {
    pub stream: WordStream,
    /// 验证状态：硬门禁
    pub verified: bool,
}

impl Module {
    pub fn from_stream(stream: WordStream) -> Self {
        Module { stream, verified: false }
    }

    /// 验证通过才置位
    pub fn mark_verified(&mut self, ok: bool) {
        self.verified = ok;
    }

    /// 产物可流出：必须已验证（fail-closed）
    pub fn can_emit(&self) -> bool {
        self.verified
    }

    /// Word 序确定性：同输入同输出（此处校验两流逐字相等）
    pub fn identical(&self, other: &Module) -> bool {
        self.stream.words == other.stream.words
    }
}

/// 能力最小化判定：仅 IR 用到的特性声明
pub fn minimal_caps(uses_image_gather: bool) -> Vec<Capability> {
    let mut caps = vec![Capability::Shader];
    if uses_image_gather {
        caps.push(Capability::ImageGatherExtended);
    }
    caps
}

/// 确定性自检：两次独立装配同输入产出逐字一致
pub fn deterministic_check(f: impl Fn() -> WordStream) -> bool {
    let a = f();
    let b = f();
    a.words == b.words
}

pub fn run_vec161_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec161");
    // 族一：Header 与魔数
    let mut ws = WordStream::new();
    s.add("头-空流非法", !ws.header_ok(), "empty");
    ws.header(0x0001_0300, 16);
    s.add("头-五字齐备", ws.header_ok() && ws.len_words() == 5, "hdr5");
    s.add("头-魔数正确", ws.words[0] == 0x0723_0203, "magic");
    s.add("头-Schema为0", ws.words[4] == 0, "schema");
    s.add("头-边界ID写入", ws.words[3] == 16, "bound");
    // 族二：指令编码 word_count
    ws.instruction(17, &[1, 32, 0]); // OpCapability Shader → 4 字
    s.add("指令-word0高16位字数", ws.words[5] == (4 << 16) | 17, "wc");
    s.add("指令-操作数依序", ws.words[6] == 1 && ws.words[8] == 0, "ops");
    // 族三：ID 单调分配
    let mut ia = IdAlloc::new();
    let a = ia.alloc();
    let b = ia.alloc();
    let c = ia.alloc();
    s.add("ID-起始为1", a == 1, "first");
    s.add("ID-单调递增", a < b && b < c, "mono");
    s.add("ID-边界自动抬升", ia.bound == 4, "bound");
    s.add("ID-未耗尽", !ia.near_exhaustion(), "ok");
    // 族四：能力最小化
    let caps0 = minimal_caps(false);
    s.add("能力-最小仅Shader", caps0.len() == 1 && caps0[0] == Capability::Shader, "min");
    let caps1 = minimal_caps(true);
    s.add("能力-按需追加", caps1.len() == 2 && caps1[1] == Capability::ImageGatherExtended, "need");
    s.add("能力-编码值", Capability::Shader.code() == 1 && Capability::ImageGatherExtended.code() == 25, "code");
    // 族五：产物封装 fail-closed
    let mut m = Module::from_stream(ws);
    s.add("封装-未验禁止流出", !m.can_emit(), "failclosed");
    m.mark_verified(true);
    s.add("封装-验证后可流出", m.can_emit(), "emit");
    m.mark_verified(false);
    s.add("封装-验证撤销再禁", !m.can_emit(), "revoke");
    // 族六：确定性
    let f = || {
        let mut w = WordStream::new();
        w.header(0x0001_0300, 8);
        w.instruction(17, &[1]);
        w
    };
    s.add("确定-两次装配一致", deterministic_check(f), "det");
    let mut m2 = Module::from_stream(f());
    let m3 = Module::from_stream(f());
    m2.mark_verified(true);
    s.add("确定-逐字比对", m2.identical(&m3), "same");
    // 族七：fail-closed 隔离——未验模块即使内容合法也不流出
    let m4 = Module::from_stream(f());
    s.add("门禁-硬门禁不受内容影响", !m4.can_emit() && m4.stream.header_ok(), "gate");
    s
}
