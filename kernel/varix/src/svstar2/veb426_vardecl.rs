//! veb426_vardecl —— VE-F0426 变量声明与限定符。
//! 锚点（CoRun Unxreal 主册）：VE-F0426 · 变量声明与限定符 · 目标 340 行
//! 判据原文：VE-F0426-J1 变量声明解析：名 + 型 + 限定符集
//!           （const/volatile/static/atomic 位集）；限定符组
//!           合合法性核（atomic 与 volatile 互斥、const 与
//!           atomic 兼容）；重声明检（同名同域 = 冲突拒，异域
//!           遮蔽记录）；域栈（块作用域进出，出域回收变量账）；
//!           限定符位哈希（型 + 位集 FNV32）。
//! 组件：限定符位集 + 声明结构 + 合法性核 + 域栈 + 遮蔽账。
//! 职责定位：变量声明解析是「C 域的名字管家」——限定符是变量
//!           的行为契约：const 是只读承诺，atomic 是并发承诺，
//!           volatile 与 atomic 互斥是语义红线（两者都在说「别
//!           优化我」但路径完全不同）；域栈是生命周期管理：出
//!           域回收，同名遮蔽有账可查。
//! 性能逐项分解：声明 O(1)；出域 O(块内)；遮蔽 O(n)。
//! 诊断码段：0x30A00（0x30A01~0x30A09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const VD_VERSION: &str = "veb426_vardecl-v1";

/// 诊断段基址（0x30A00 段 · 本模块独占）。
pub const VD_SEG_BASE: u32 = 0x30A00;
pub const XC_QUAL_CONFL: u32 = 0x30A01; // 限定符冲突
pub const XC_REDECL: u32 = 0x30A02; // 同域重声明
pub const XC_BAD_TYPE: u32 = 0x30A03; // 型不合法
pub const XC_MAGIC: u32 = 0x30A04; // 账魔数破
pub const XC_NOT_DETER: u32 = 0x30A05; // 解析不确定
pub const XC_NAME_EMPTY: u32 = 0x30A06; // 名空
pub const XC_SCOPE: u32 = 0x30A07; // 域操作错
pub const XC_QUAL_OOR: u32 = 0x30A08; // 限定符越 8 位
pub const XC_SHAD: u32 = 0x30A09; // 遮蔽记录不一致

/// 限定符位。
pub const Q_CONST: u8 = 1 << 0;
pub const Q_VOLATILE: u8 = 1 << 1;
pub const Q_STATIC: u8 = 1 << 2;
pub const Q_ATOMIC: u8 = 1 << 3;

/// 型字。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VT {
    /// u8。
    U8 = 0,
    /// u32。
    U32 = 1,
    /// u64。
    U64 = 2,
    /// 指针。
    Ptr = 3,
}

impl VT {
    pub fn from_u32(v: u32) -> Option<VT> {
        match v {
            0 => Some(VT::U8),
            1 => Some(VT::U32),
            2 => Some(VT::U64),
            3 => Some(VT::Ptr),
            _ => None,
        }
    }
    /// 型宽（字节）。
    pub fn width(self) -> u32 {
        match self {
            VT::U8 => 1,
            VT::U32 => 4,
            VT::U64 => 8,
            VT::Ptr => 8,
        }
    }
}

/// 限定符组合合法性核。
pub fn qual_ok(q: u8) -> bool {
    // volatile 与 atomic 互斥。
    if q & Q_VOLATILE != 0 && q & Q_ATOMIC != 0 {
        return false;
    }
    // const 与 atomic 兼容（合法组合）。
    true
}

/// 变量声明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarDecl {
    pub name: &'static str,
    pub ty: VT,
    pub quals: u8,
    pub depth: u32,
}

fn fnv32v(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl VarDecl {
    /// 限定符位哈希（型 + 位集）。
    pub fn qhash(&self) -> u32 {
        let buf = [self.ty as u8, self.quals];
        fnv32v(&buf)
    }
}

/// 遮蔽记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadRec {
    pub name: &'static str,
    pub outer_depth: u32,
    pub inner_depth: u32,
}

/// 域栈解析器。
pub struct VarScope {
    /// 当前可见变量（带深度）。
    vars: Vec<VarDecl>,
    depth: u32,
    shads: Vec<ShadRec>,
    magic: u32,
}

impl VarScope {
    /// 建。
    pub fn new() -> VarScope {
        VarScope { vars: Vec::new(), depth: 0, shads: Vec::new(), magic: 0x0A00_5A12 }
    }

    /// 声明。
    pub fn declare(&mut self, name: &'static str, ty: VT, quals: u8) -> Result<VarDecl, u32> {
        if self.magic != 0x0A00_5A12 {
            return Err(XC_MAGIC);
        }
        if name.is_empty() {
            return Err(XC_NAME_EMPTY);
        }
        if !qual_ok(quals) {
            return Err(XC_QUAL_CONFL);
        }
        // 同域重声明拒。
        for v in self.vars.iter() {
            if v.name == name && v.depth == self.depth {
                return Err(XC_REDECL);
            }
        }
        // 异域同名 = 遮蔽（记录外层深度）。
        let mut outer: Option<u32> = None;
        for v in self.vars.iter() {
            if v.name == name && v.depth < self.depth {
                outer = Some(v.depth);
            }
        }
        if let Some(od) = outer {
            self.shads.push(ShadRec { name, outer_depth: od, inner_depth: self.depth });
        }
        let d = VarDecl { name, ty, quals, depth: self.depth };
        self.vars.push(d.clone());
        Ok(d)
    }

    /// 进块。
    pub fn push_scope(&mut self) {
        self.depth += 1;
    }

    /// 出块（回收块内变量）。
    pub fn pop_scope(&mut self) -> Result<u32, u32> {
        if self.depth == 0 {
            return Err(XC_SCOPE);
        }
        let d = self.depth;
        self.vars.retain(|v| v.depth < d);
        self.depth -= 1;
        Ok(self.vars.len() as u32)
    }

    /// 查可见变量（最近声明 = 最内层）。
    pub fn lookup(&self, name: &str) -> Result<VarDecl, u32> {
        let mut best: Option<&VarDecl> = None;
        for v in self.vars.iter() {
            if v.name == name {
                match best {
                    Some(b) if b.depth > v.depth => {}
                    _ => best = Some(v),
                }
            }
        }
        match best {
            Some(v) => Ok(v.clone()),
            None => Err(XC_REDECL), // 查无（借用码位；语义为未声明）
        }
    }

    /// 遮蔽账。
    pub fn shadow_log(&self) -> Vec<ShadRec> {
        self.shads.clone()
    }

    /// 深度。
    pub fn cur_depth(&self) -> u32 {
        self.depth
    }

    /// 可见变量数。
    pub fn visible(&self) -> usize {
        self.vars.len()
    }
}

pub fn run_veb426_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0426");
    // 族一：限定符组合。
    s.add("纯 const 过", qual_ok(Q_CONST), "合法");
    s.add("const+atomic 过", qual_ok(Q_CONST | Q_ATOMIC), "兼容");
    s.add("volatile+atomic 冲突", !qual_ok(Q_VOLATILE | Q_ATOMIC), "互斥红线");
    s.add("纯 volatile 过", qual_ok(Q_VOLATILE), "单走合法");
    s.add("全无过", qual_ok(0), "裸变量");
    // 族二：声明与重声明。
    let mut sc = VarScope::new();
    let d1 = sc.declare("x", VT::U32, Q_CONST);
    s.add("声明过", d1.is_ok(), "x 入域");
    s.add("同域重声明拒", sc.declare("x", VT::U32, 0) == Err(XC_REDECL), "冲突");
    s.add("名空拒", sc.declare("", VT::U8, 0) == Err(XC_NAME_EMPTY), "白名");
    s.add("冲突限定拒", sc.declare("y", VT::U8, Q_VOLATILE | Q_ATOMIC) == Err(XC_QUAL_CONFL), "红线");
    // 族三：域栈与遮蔽。
    sc.push_scope();
    s.add("深度 1", sc.cur_depth() == 1, "进块");
    let d2 = sc.declare("x", VT::U64, Q_ATOMIC);
    s.add("内层遮蔽过", d2.is_ok(), "异域同名");
    let look = sc.lookup("x");
    s.add("内层优先", matches!(&look, Ok(v) if v.ty == VT::U64 && v.depth == 1), "遮蔽生效");
    s.add("遮蔽账 1", sc.shadow_log().len() == 1, "一遮");
    s.add("遮蔽账内容", matches!(sc.shadow_log().first(), Some(r) if r.name == "x" && r.outer_depth == 0 && r.inner_depth == 1), "0→1");
    let after_pop = sc.pop_scope();
    s.add("出块回收", after_pop == Ok(1), "剩外层 x");
    let look2 = sc.lookup("x");
    s.add("外层复见", matches!(&look2, Ok(v) if v.ty == VT::U32 && v.depth == 0), "遮蔽解除");
    s.add("底出块拒", sc.pop_scope() == Err(XC_SCOPE), "域栈空");
    // 族四：限定符哈希。
    let mut sc2 = VarScope::new();
    let a1 = sc2.declare("a", VT::U32, Q_CONST);
    let a2 = sc2.declare("b", VT::U32, Q_CONST);
    let a3 = sc2.declare("c", VT::U32, 0);
    let h1 = match (&a1, &a2, &a3) {
        (Ok(x), Ok(y), Ok(z)) => (x.qhash(), y.qhash(), z.qhash()),
        _ => { s.add("声明败", false, "不应到"); return s; }
    };
    s.add("同型同限同哈希", h1.0 == h1.1, "位集定哈希");
    s.add("异限异哈希", h1.0 != h1.2, "const 位参与");
    // 型宽。
    s.add("型宽核", VT::U8.width() == 1 && VT::U64.width() == 8 && VT::Ptr.width() == 8, "1/4/8");
    // 族五：型枚举。
    s.add("型闭集", VT::from_u32(3) == Some(VT::Ptr) && VT::from_u32(4).is_none(), "四型");
    s.add("位集界", Q_CONST | Q_VOLATILE | Q_STATIC | Q_ATOMIC == 0x0F, "4 位");
    s.add("魔数核", VD_SEG_BASE == 0x30A00, "段基址");
    s
}
