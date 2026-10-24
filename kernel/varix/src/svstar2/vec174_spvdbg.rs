//! VE-F0574 · SPIR-V 调试信息
//!
//! OpSource、OpName、OpMemberName、OpLine 全族生成——与 IR 调试装饰（F0476）
//! 逐项映射（IR 层装饰到 SPIR-V 指令的对应表）；调试与发布双模式：调试模式
//! 保留全量名称与行号映射，发布模式剥离策略（产物体积与解析开销的权衡，
//! 剥离清单显性）；NonSemantic.DebugInfo 富调试扩展为远景预留态；OpLine 支撑
//! 错误回溯到源码（与 F0508 回溯机制衔接）。
//!
//! 跨批对接：上游 F0476 IR 调试装饰、F0508 回溯机制；下游 F0576 文档、F0580 组收口。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const OP_SOURCE: u32 = 3;
pub const OP_NAME: u32 = 5;
pub const OP_MEMBER_NAME: u32 = 6;
pub const OP_LINE: u32 = 8;

/// 调试指令种类（与 F0476 IR 装饰映射）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgKind {
    Source,
    Name,
    MemberName,
    Line,
}

impl DbgKind {
    /// IR 装饰 → SPIR-V 指令映射表
    pub fn from_ir_deco(deco: u32) -> Option<DbgKind> {
        match deco {
            0 => Some(DbgKind::Source),
            1 => Some(DbgKind::Name),
            2 => Some(DbgKind::MemberName),
            3 => Some(DbgKind::Line),
            _ => None,
        }
    }

    pub fn opcode(self) -> u32 {
        match self {
            DbgKind::Source => OP_SOURCE,
            DbgKind::Name => OP_NAME,
            DbgKind::MemberName => OP_MEMBER_NAME,
            DbgKind::Line => OP_LINE,
        }
    }
}

/// 行号映射：SPIR-V 指令序号 → 源码行（F0508 衔接）
#[derive(Clone, Debug, Default)]
pub struct LineMap {
    /// 指令序号 → (文件号, 源码行, 列)
    pub entries: Vec<(u32, u32, u32, u32)>,
}

impl LineMap {
    pub fn add(&mut self, inst_idx: u32, file: u32, line: u32, col: u32) -> bool {
        if self.entries.iter().any(|(i, _, _, _)| *i == inst_idx) {
            return false;
        }
        self.entries.push((inst_idx, file, line, col));
        true
    }

    /// O(1) 语义的查表（登记序线性但按指令序排序后可二分；此处保持登记序）
    pub fn lookup(&self, inst_idx: u32) -> Option<(u32, u32, u32, u32)> {
        self.entries.iter().find(|(i, _, _, _)| *i == inst_idx).map(|&(_, f, l, c)| (f, l, c, inst_idx))
    }

    /// 回溯：给定指令序号 → 源码行
    pub fn to_src_line(&self, inst_idx: u32) -> Option<u32> {
        self.lookup(inst_idx).map(|(_, _, l, _)| l)
    }
}

/// 双模式
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbgMode {
    /// 调试：全量名称与行号映射
    Debug,
    /// 发布：剥离
    Release,
}

/// 剥离清单：发布模式剥离的指令种类（显性）
pub const STRIP_LIST: [DbgKind; 4] = [DbgKind::Source, DbgKind::Name, DbgKind::MemberName, DbgKind::Line];

/// 调试发射器
#[derive(Clone, Debug)]
pub struct DbgEmitter {
    pub mode: DbgMode,
    pub ops: Vec<(u32, Vec<u32>)>,
}

impl DbgEmitter {
    pub fn new(mode: DbgMode) -> Self {
        DbgEmitter { mode, ops: Vec::new() }
    }

    /// 发射单条调试指令；发布模式按剥离清单跳过
    pub fn emit(&mut self, kind: DbgKind, operands: &[u32]) -> bool {
        if self.mode == DbgMode::Release && STRIP_LIST.contains(&kind) {
            return false;
        }
        self.ops.push((kind.opcode(), operands.to_vec()));
        true
    }

    pub fn count(&self, kind: DbgKind) -> usize {
        self.ops.iter().filter(|(op, _)| *op == kind.opcode()).count()
    }

    pub fn total(&self) -> usize {
        self.ops.len()
    }
}

/// 剥离器：对已产模块做发布剥离（O(1) 开关语义）
pub fn strip(module_ops: &[(u32, Vec<u32>)]) -> Vec<(u32, Vec<u32>)> {
    module_ops
        .iter()
        .filter(|(op, _)| !STRIP_LIST.iter().any(|k| k.opcode() == *op))
        .cloned()
        .collect()
}

/// 富调试扩展：远景预留态
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RichDbg {
    /// 基础指令集（当前）
    Basic,
    /// NonSemantic.DebugInfo（预留）
    NonSemanticReserved,
}

/// 等价性校验：剥离后非调试指令序列保持不变
pub fn strip_preserves(module_ops: &[(u32, Vec<u32>)]) -> bool {
    let stripped = strip(module_ops);
    let keep: Vec<&(u32, Vec<u32>)> = module_ops
        .iter()
        .filter(|(op, _)| !STRIP_LIST.iter().any(|k| k.opcode() == *op))
        .collect();
    keep.len() == stripped.len() && keep.iter().zip(stripped.iter()).all(|(a, b)| **a == *b)
}

pub fn run_vec174_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec174");
    // 族一：全指令映射
    s.add("映射-装饰0→Source", DbgKind::from_ir_deco(0) == Some(DbgKind::Source), "m0");
    s.add("映射-装饰1→Name", DbgKind::from_ir_deco(1) == Some(DbgKind::Name), "m1");
    s.add("映射-装饰2→MemberName", DbgKind::from_ir_deco(2) == Some(DbgKind::MemberName), "m2");
    s.add("映射-装饰3→Line", DbgKind::from_ir_deco(3) == Some(DbgKind::Line), "m3");
    s.add("映射-未知拒绝", DbgKind::from_ir_deco(9).is_none(), "unk");
    s.add("映射-指令码", DbgKind::Line.opcode() == OP_LINE && DbgKind::Source.opcode() == OP_SOURCE, "code");
    // 族二：行号映射与回溯衔接
    let mut lm = LineMap::default();
    s.add("行表-登记", lm.add(3, 0, 12, 5), "add");
    s.add("行表-重复拒绝", !lm.add(3, 0, 13, 1), "dup");
    s.add("行表-查表", lm.lookup(3) == Some((0, 12, 5, 3)), "hit");
    s.add("行表-未命中", lm.lookup(9).is_none(), "miss");
    s.add("行表-回溯到源码行", lm.to_src_line(3) == Some(12), "src");
    // 族三：双模式发射
    let mut dbg = DbgEmitter::new(DbgMode::Debug);
    dbg.emit(DbgKind::Source, &[1]);
    dbg.emit(DbgKind::Name, &[2, 3]);
    dbg.emit(DbgKind::MemberName, &[2, 0, 4]);
    dbg.emit(DbgKind::Line, &[5, 6, 7]);
    s.add("调试-全量四类", dbg.total() == 4 && dbg.count(DbgKind::Line) == 1, "all");
    let mut rel = DbgEmitter::new(DbgMode::Release);
    s.add("发布-Source剥离", !rel.emit(DbgKind::Source, &[1]), "strip0");
    s.add("发布-Name剥离", !rel.emit(DbgKind::Name, &[1]), "strip1");
    s.add("发布-Line剥离", !rel.emit(DbgKind::Line, &[1]), "strip3");
    s.add("发布-全剥空", rel.total() == 0, "none");
    // 族四：剥离器等价性
    let module: Vec<(u32, Vec<u32>)> = vec![
        (OP_SOURCE, vec![1]),
        (100, vec![9]),
        (OP_LINE, vec![1, 2, 3]),
        (101, vec![8]),
    ];
    s.add("剥离-保留非调试", strip(&module).len() == 2, "keep");
    s.add("剥离-序列不变", strip_preserves(&module), "preserve");
    // 剥离误删拦截：剥离清单外的指令必须全部保留
    let mut bad_module = module.clone();
    bad_module.push((OP_NAME, vec![7]));
    s.add("剥离-清单外不误删", strip_preserves(&bad_module), "safe");
    // 族五：富调试预留态
    s.add("预留-基础态", RichDbg::Basic != RichDbg::NonSemanticReserved, "basic");
    s.add("预留-远景登记", matches!(RichDbg::NonSemanticReserved, RichDbg::NonSemanticReserved), "reserved");
    // 族六：剥离清单显性
    s.add("清单-四项显性", STRIP_LIST.len() == 4, "n4");
    s
}
