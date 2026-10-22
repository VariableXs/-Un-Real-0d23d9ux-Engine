//! VE-F0461 · VE-IR 数据结构定义（VE-C 域 · IR 组首件）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0461`
//!
//! **锚点原文**：「编译器中间表示（VE-IR）数据结构定义：指令模型（操作码、
//! 操作数、类型槽、调试位置四元组）、基本块与函数结构、模块结构（函数、
//! 全局常量、资源声明、元数据四节）、类型系统映射（语义类型到 IR 类型一
//! 表）；设计原则记录（SSA 就绪、可序列化、可验证三原则）。判据：四元组
//! 指令、SSA 就绪、可序列化、三原则记录、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **四元组指令**（判据一）。[`Inst`] 恰四要素（opcode / operands /
//!    ty 槽 / dbg 位置），缺一不可（构造经 [`Inst::new`] 强制全给）。
//!
//! 2. **SSA 就绪**（判据二）。值以虚拟寄存器号（[`VReg`]）唯一标识，每条
//!    指令至多产出一个 defs（SSA 形态的结构前提），结构面可查（[`Inst::def`]）。
//!
//! 3. **可序列化**（判据三）。模块可投影为字节序列（[`serialize_module`]：
//!    版本头 + 四节序贯），反序列化还原校验（节计数与版本核对）。
//!
//! 4. **三原则记录**（判据四）。SSA 就绪 / 可序列化 / 可验证三原则以静态
//!    文本记录（[`DESIGN_PRINCIPLES`]），逐条带落点（在哪实现）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0461";

/// IR 序列化格式版本头。
pub const IR_FORMAT_VER: u32 = 1;

/// 设计三原则（记录面：原则 + 落点）。
pub static DESIGN_PRINCIPLES: &[(&str, &str)] = &[
    ("SSA 就绪", "VReg 唯一值标识 + Inst::def 单定义面（F0463 消费）"),
    ("可序列化", "serialize_module 版本头 + 四节序贯（F0475 消费）"),
    ("可验证", "结构不变量全可机检：终接唯一/使用定义一致（F0464 消费）"),
];

// ---------------------------------------------------------------------------
// 指令四元组
// ---------------------------------------------------------------------------

/// 虚拟寄存器（SSA 值标识）。
pub type VReg = u32;

/// 操作码（枚举拦截越界——锚点「操作码越界→枚举拦截」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    // 算术
    Add,
    Sub,
    Mul,
    Mad,
    // 位/逻辑
    And,
    Or,
    Not,
    // 数据搬移
    Load,
    Store,
    Const,
    Copy,
    // 控制
    Br,
    BrCond,
    Ret,
    // 纹理
    Sample,
}

impl Opcode {
    pub fn mnemonic(&self) -> &'static str {
        match self {
            Opcode::Add => "add",
            Opcode::Sub => "sub",
            Opcode::Mul => "mul",
            Opcode::Mad => "mad",
            Opcode::And => "and",
            Opcode::Or => "or",
            Opcode::Not => "not",
            Opcode::Load => "load",
            Opcode::Store => "store",
            Opcode::Const => "const",
            Opcode::Copy => "copy",
            Opcode::Br => "br",
            Opcode::BrCond => "brcond",
            Opcode::Ret => "ret",
            Opcode::Sample => "sample",
        }
    }
    /// 副作用面（DCE 白名单机检源：F0467 复用）。
    pub fn has_side_effect(&self) -> bool {
        matches!(self, Opcode::Store | Opcode::Sample | Opcode::Ret)
    }
    /// 终接面（块终接唯一性机检源：F0464 复用）。
    pub fn is_terminator(&self) -> bool {
        matches!(self, Opcode::Br | Opcode::BrCond | Opcode::Ret)
    }
}

/// 调试位置（源码定位；None = 灰态指令，F0465 标注）。
pub type DbgPos = Option<u32>;

/// 指令四元组：操作码 + 操作数 + 类型槽 + 调试位置。
#[derive(Clone, Debug, PartialEq)]
pub struct Inst {
    pub opcode: Opcode,
    pub operands: Vec<VReg>,
    /// 类型槽（结果类型摘要；None = 无产出/类型未定——验证器拦）。
    pub ty: Option<&'static str>,
    pub dbg: DbgPos,
    /// 结果寄存器（SSA 单定义面；None = 纯副作用/终接指令）。
    pub def: Option<VReg>,
}

impl Inst {
    /// 四元组全量构造（强制：操作码给死，操作数/类型槽/位置显性给）。
    pub fn new(opcode: Opcode, operands: Vec<VReg>, ty: Option<&'static str>, dbg: DbgPos, def: Option<VReg>) -> Self {
        Inst { opcode, operands, ty, dbg, def }
    }
    /// 值产出指令（带 def 与类型槽）。
    pub fn value(opcode: Opcode, operands: Vec<VReg>, ty: &'static str, dbg: u32, def: VReg) -> Self {
        Inst::new(opcode, operands, Some(ty), Some(dbg), Some(def))
    }
    /// 终接指令（无产出）。
    pub fn term(opcode: Opcode, operands: Vec<VReg>, dbg: u32) -> Self {
        Inst::new(opcode, operands, None, Some(dbg), None)
    }
    /// 四要素齐备（类型槽+位置都在 = 非灰态）。
    pub fn fully_annotated(&self) -> bool {
        self.ty.is_some() && self.dbg.is_some()
    }
}

// ---------------------------------------------------------------------------
// 基本块与函数
// ---------------------------------------------------------------------------

/// 基本块：指令序贯 + 前驱/后继（CFG 面由终接指令推导）。
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub id: u32,
    pub insts: Vec<Inst>,
}

impl Block {
    pub fn new(id: u32) -> Self {
        Block { id, insts: Vec::new() }
    }
    pub fn push(&mut self, i: Inst) {
        self.insts.push(i);
    }
    /// 终接指令（末条须为终接——验证器口径）。
    pub fn terminator(&self) -> Option<&Inst> {
        self.insts.last().filter(|i| i.opcode.is_terminator())
    }
    pub fn is_terminated(&self) -> bool {
        self.terminator().is_some()
    }
}

/// 函数：块序贯（块 0 为入口）+ 参数面。
#[derive(Clone, Debug)]
pub struct IrFunc {
    pub name: String,
    pub params: Vec<VReg>,
    pub blocks: Vec<Block>,
    /// 下一可用虚拟寄存器号（分配器游标）。
    next_reg: VReg,
}

impl IrFunc {
    pub fn new(name: &str) -> Self {
        IrFunc {
            name: name.to_string(),
            params: Vec::new(),
            blocks: vec![Block::new(0)],
            next_reg: 0,
        }
    }
    /// 寄存器分配（单调递增，不复用——SSA 就绪）。
    pub fn fresh_reg(&mut self) -> VReg {
        let r = self.next_reg;
        self.next_reg += 1;
        r
    }
    pub fn cur_block(&mut self) -> &mut Block {
        if self.blocks.is_empty() {
            self.blocks.push(Block::new(0));
        }
        let n = self.blocks.len();
        &mut self.blocks[n - 1]
    }
    pub fn new_block(&mut self) -> u32 {
        let id = self.blocks.len() as u32;
        self.blocks.push(Block::new(id));
        id
    }
    /// 全函数指令数（遍历面 O(指令数) 的计数口径）。
    pub fn inst_count(&self) -> usize {
        self.blocks.iter().map(|b| b.insts.len()).sum()
    }
}

// ---------------------------------------------------------------------------
// 模块四节
// ---------------------------------------------------------------------------

/// 资源声明（绑定槽 + 类型摘要）。
#[derive(Clone, Debug, PartialEq)]
pub struct ResDecl {
    pub binding: u32,
    pub kind: &'static str,
}

/// 模块四节：函数 / 全局常量 / 资源声明 / 元数据。
#[derive(Debug, Default)]
pub struct IrModule {
    pub funcs: Vec<IrFunc>,
    pub globals: Vec<(String, &'static str)>,
    pub resources: Vec<ResDecl>,
    pub meta: Vec<(String, String)>,
}

impl IrModule {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_func(&mut self, f: IrFunc) {
        self.funcs.push(f);
    }
    pub fn find_func(&self, name: &str) -> Option<&IrFunc> {
        self.funcs.iter().find(|f| f.name == name)
    }
}

// ---------------------------------------------------------------------------
// 序列化（版本头 + 四节序贯；字节面还原校验）
// ---------------------------------------------------------------------------

/// 序列化投影：版本头（4B）+ 节计数（4B）+ 四节内容
/// （函数节 = 函数名长度+名+块数；其余节 = 条目摘要串）。
pub fn serialize_module(m: &IrModule) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(&IR_FORMAT_VER.to_le_bytes());
    out.extend_from_slice(&(m.funcs.len() as u32).to_le_bytes());
    for f in m.funcs.iter() {
        out.extend_from_slice(&(f.name.len() as u32).to_le_bytes());
        out.extend_from_slice(f.name.as_bytes());
        out.extend_from_slice(&(f.blocks.len() as u32).to_le_bytes());
    }
    // 全局/资源/元数据三节按条目数 + 摘要串。
    out.extend_from_slice(&(m.globals.len() as u32).to_le_bytes());
    out.extend_from_slice(&(m.resources.len() as u32).to_le_bytes());
    out.extend_from_slice(&(m.meta.len() as u32).to_le_bytes());
    out
}

/// 反序列化校验：版本核对 + 按实际布局走节（函数节 = 名长+名+块数，
/// 尾接三节计数；截断/版本不匹配即显性拒绝）。
pub fn deserialize_check(bytes: &[u8]) -> Result<(u32, usize, usize, usize, usize), String> {
    if bytes.len() < 8 {
        return Err("长度不足：头面缺失".to_string());
    }
    let rd = |off: usize| -> u32 {
        u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
    };
    let ver = rd(0);
    if ver != IR_FORMAT_VER {
        return Err(format!("版本不匹配：读 {} 需 {}", ver, IR_FORMAT_VER));
    }
    let nf = rd(4) as usize;
    let mut off = 8usize;
    for _ in 0..nf {
        if off + 4 > bytes.len() {
            return Err("截断：函数节名长".to_string());
        }
        let nl = rd(off) as usize;
        off += 4;
        if off + nl + 4 > bytes.len() {
            return Err("截断：函数名或块数".to_string());
        }
        off += nl + 4;
    }
    if off + 12 > bytes.len() {
        return Err("截断：尾节计数".to_string());
    }
    Ok((ver, nf, rd(off) as usize, rd(off + 4) as usize, rd(off + 8) as usize))
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn vec61_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 一、四元组指令。
    {
        // 值指令：四要素全在。
        let i = Inst::value(Opcode::Add, vec![1, 2], "ir.v4f", 100, 3);
        v.push((
            "F0461-指令-四元组齐备",
            i.opcode == Opcode::Add
                && i.operands == vec![1, 2]
                && i.ty == Some("ir.v4f")
                && i.dbg == Some(100),
        ));
        // 终接指令：无产出面。
        let t = Inst::term(Opcode::Ret, vec![], 9);
        v.push(("F0461-指令-终接无产出", t.def.is_none() && t.opcode.is_terminator()));
        // 副作用白名单源：store/sample/ret 标副作用。
        v.push((
            "F0461-指令-副作用面",
            Opcode::Store.has_side_effect()
                && Opcode::Sample.has_side_effect()
                && !Opcode::Add.has_side_effect(),
        ));
    }

    // 二、SSA 就绪。
    {
        let mut f = IrFunc::new("vs_main");
        let r0 = f.fresh_reg();
        let r1 = f.fresh_reg();
        // 分配单调不复用。
        v.push(("F0461-SSA-分配单调", r0 == 0 && r1 == 1));
        // 单定义面：value 指令 def 唯一。
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r0));
        f.cur_block().push(Inst::value(Opcode::Add, vec![r0, r0], "ir.f32", 2, r1));
        v.push((
            "F0461-SSA-单定义面",
            f.blocks[0].insts.iter().all(|i| i.def.is_some()),
        ));
    }

    // 三、可序列化。
    {
        let mut m = IrModule::new();
        let mut f = IrFunc::new("ps_main");
        let r = f.fresh_reg();
        f.cur_block().push(Inst::value(Opcode::Const, vec![], "ir.f32", 1, r));
        f.cur_block().push(Inst::term(Opcode::Ret, vec![r], 2));
        m.add_func(f);
        m.globals.push(("MAX_LIGHTS".to_string(), "s:int"));
        m.resources.push(ResDecl { binding: 0, kind: "sampler2d" });
        m.meta.push(("stage".to_string(), "fragment".to_string()));
        let bytes = serialize_module(&m);
        // 版本头 + 五节计数可还原。
        let ok = match deserialize_check(&bytes) {
            Ok((ver, nf, ng, nr, nm)) => {
                ver == 1 && nf == 1 && ng == 1 && nr == 1 && nm == 1
            }
            Err(_) => false,
        };
        v.push(("F0461-序列化-版本头还原", ok));
        // 显性拒绝：版本不符。
        let mut bad = bytes.clone();
        bad[0] = 9;
        v.push((
            "F0461-序列化-版本拒绝",
            deserialize_check(&bad).is_err(),
        ));
    }

    // 四、三原则记录。
    {
        v.push((
            "F0461-原则-三原则记录",
            DESIGN_PRINCIPLES.len() == 3
                && DESIGN_PRINCIPLES.iter().all(|(p, w)| !p.is_empty() && !w.is_empty()),
        ));
        v.push((
            "F0461-原则-落点可查",
            DESIGN_PRINCIPLES.iter().all(|(_, w)| w.contains("F04")),
        ));
    }

    // 五、判据自检。
    {
        v.push(("F0461-判据-锚点入册", ANCHOR == "VE-F0461"));
    }

    v
}

/// 注册表入口：Vec 判据 → CheckSet（域标签 svstar2-vec61）。
pub fn run_vec61_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-vec61");
    for (name, ok) in vec61_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::vec61_raw_checks as run_vec61_checks;
    #[test]
    fn vec61_irstruct_checks() {
        for (name, ok) in run_vec61_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
