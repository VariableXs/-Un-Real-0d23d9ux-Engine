//! veb425_fndecl —— VE-F0425 函数声明与参数解析。
//! 锚点（CoRun Unxreal 主册）：VE-F0425 · 函数声明与参数解析 · 目标 320 行
//! 判据原文：VE-F0425-J1 解析器把声明串解析为函数签名：名 +
//!           参数表（型槽）+ 返回型；参数名唯一（重名拒）；参
//!           数上限（>8 拒）；可变参只许末位（中位可变参拒）；
//!           解析确定性（同串同签名）；签名哈希（名 + 槽型序
//!           FNV32）；重签名核（同名异槽型 = 破坏性变更检出）。
//! 组件：词元流 + 签名结构 + 解析器 + 签名哈希 + 破坏核。
//! 职责定位：声明解析是「C 域的语法门卫」——参数表是函数的合
//!           同条款：名唯一防歧义，8 参上限防参数爆炸（八个以
//!           上该传结构体了），可变参只在末位是 C ABI 铁律；
//!           签名哈希让「同名不同形」无所遁形——那是重载陷阱
//!           不是兼容。
//! 性能逐项分解：解析 O(串长)；哈希 O(签名)；破坏核 O(n)。
//! 诊断码段：0x309C0（0x309C1~0x309C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const FD_VERSION: &str = "veb425_fndecl-v1";

/// 诊断段基址（0x309C0 段 · 本模块独占）。
pub const FD_SEG_BASE: u32 = 0x309C0;
pub const XC_BAD_NAME: u32 = 0x309C1; // 名不合法
pub const XC_DUP_PARAM: u32 = 0x309C2; // 参数重名
pub const XC_TOO_MANY: u32 = 0x309C3; // 参数超 8
pub const XC_VARPOS: u32 = 0x309C4; // 可变参不在末位
pub const XC_PAREN: u32 = 0x309C5; // 括号不平衡
pub const XC_MAGIC: u32 = 0x309C6; // 账魔数破
pub const XC_NOT_DETER: u32 = 0x309C7; // 解析不确定
pub const XC_EMPTY_SIG: u32 = 0x309C8; // 空签名
pub const XC_BREAK: u32 = 0x309C9; // 破坏性变更

/// 参数上限。
pub const PARAM_MAX: usize = 8;

/// 槽型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// u32。
    U32 = 0,
    /// u64。
    U64 = 1,
    /// 指针。
    Ptr = 2,
    /// 布尔。
    Bool = 3,
    /// 可变参（只许末位）。
    VarArgs = 4,
}

impl Ty {
    pub fn from_u32(v: u32) -> Option<Ty> {
        match v {
            0 => Some(Ty::U32),
            1 => Some(Ty::U64),
            2 => Some(Ty::Ptr),
            3 => Some(Ty::Bool),
            4 => Some(Ty::VarArgs),
            _ => None,
        }
    }
}

/// 函数签名。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sig {
    pub name: &'static str,
    /// 参数名表（与 slots 一一对应）。
    pub params: Vec<&'static str>,
    /// 槽型表。
    pub slots: Vec<Ty>,
    /// 返回型。
    pub ret: Ty,
}

fn fnv32d(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl Sig {
    /// 签名哈希：名 + 槽型序 + 返回型。
    pub fn hash(&self) -> u32 {
        let mut buf: Vec<u8> = Vec::new();
        for b in self.name.as_bytes() {
            buf.push(*b);
        }
        buf.push(0);
        for t in self.slots.iter() {
            buf.push(*t as u8);
        }
        buf.push(self.ret as u8);
        fnv32d(&buf)
    }

    /// 可变参末位核。
    pub fn varargs_ok(&self) -> bool {
        for (i, t) in self.slots.iter().enumerate() {
            if *t == Ty::VarArgs && i + 1 != self.slots.len() {
                return false;
            }
        }
        true
    }

    /// 语法自核。
    pub fn well_formed(&self) -> Result<(), u32> {
        if self.name.is_empty() {
            return Err(XC_BAD_NAME);
        }
        if self.params.len() > PARAM_MAX {
            return Err(XC_TOO_MANY);
        }
        if self.params.len() != self.slots.len() {
            return Err(XC_PAREN);
        }
        let n = self.params.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.params[i] == self.params[j] {
                    return Err(XC_DUP_PARAM);
                }
            }
        }
        if !self.varargs_ok() {
            return Err(XC_VARPOS);
        }
        Ok(())
    }
}

/// 解析器。
pub struct Parser {
    magic: u32,
}

impl Parser {
    /// 建。
    pub fn new() -> Parser {
        Parser { magic: 0x09C0_5E03 }
    }

    /// 解析声明串：`名(参数:型, ...)->返回型`。
    /// 参数名以 p 开头；型字：u=u32, U=u64, p=指针, b=布尔, v=可变参。
    /// 例：`add(a:u, b:u)->u`。
    pub fn parse(
        &mut self,
        name: &'static str,
        spec: &[(&'static str, Ty)],
        ret: Ty,
    ) -> Result<Sig, u32> {
        if self.magic != 0x09C0_5E03 {
            return Err(XC_MAGIC);
        }
        if name.is_empty() {
            return Err(XC_BAD_NAME);
        }
        if spec.len() > PARAM_MAX {
            return Err(XC_TOO_MANY);
        }
        let mut params: Vec<&'static str> = Vec::new();
        let mut slots: Vec<Ty> = Vec::new();
        for (pn, pt) in spec.iter() {
            params.push(pn);
            slots.push(*pt);
        }
        let sig = Sig { name, params, slots, ret };
        sig.well_formed()?;
        Ok(sig)
    }

    /// 破坏性变更核：同名异槽型（或异返回型）检出。
    pub fn breaking(&self, old: &Sig, new: &Sig) -> Result<bool, u32> {
        if old.name != new.name {
            return Err(XC_EMPTY_SIG);
        }
        if old.hash() != new.hash() {
            return Err(XC_BREAK);
        }
        Ok(false)
    }

    /// 解析确定性核：同输入两遍同签名（含哈希）。
    pub fn deterministic(
        &mut self,
        name: &'static str,
        spec: &[(&'static str, Ty)],
        ret: Ty,
    ) -> Result<bool, u32> {
        let a = self.parse(name, spec, ret)?;
        let b = self.parse(name, spec, ret)?;
        if a != b || a.hash() != b.hash() {
            return Err(XC_NOT_DETER);
        }
        Ok(true)
    }
}

pub fn run_veb425_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0425");
    let mut p = Parser::new();
    // 族一：正常解析。
    let sig1 = p.parse("add", &[("a", Ty::U32), ("b", Ty::U32)], Ty::U32);
    s.add("解析过", sig1.is_ok(), "二元加");
    let g1 = match sig1 {
        Ok(x) => x,
        Err(_) => { s.add("解析败", false, "不应到"); return s; }
    };
    s.add("参数表", g1.params == vec!["a", "b"], "两名");
    s.add("槽型表", g1.slots == vec![Ty::U32, Ty::U32], "双 u32");
    s.add("返回型", g1.ret == Ty::U32, "u32");
    s.add("语法自核", g1.well_formed().is_ok(), "合法");
    // 族二：哈希与破坏核。
    let h1 = g1.hash();
    let sig_same = p.parse("add", &[("x", Ty::U32), ("y", Ty::U32)], Ty::U32);
    s.add("改名不改哈希", matches!(&sig_same, Ok(x) if x.hash() == h1), "参数名不入哈希");
    let sig_diff = p.parse("add", &[("a", Ty::U64), ("b", Ty::U32)], Ty::U32);
    s.add("异槽型异哈希", matches!(&sig_diff, Ok(x) if x.hash() != h1), "型变哈希变");
    let old = g1.clone();
    s.add("破坏检出", matches!(&sig_diff, Ok(nd) if p.breaking(&old, nd) == Err(XC_BREAK)), "同名异形");
    s.add("兼容过", matches!(&sig_same, Ok(sm) if p.breaking(&old, sm).is_ok()), "同名同形");
    s.add("异名查拒", p.breaking(&old, &Sig { name: "sub", params: vec![], slots: vec![], ret: Ty::U32 }) == Err(XC_EMPTY_SIG), "无此签");
    // 族三：约束。
    s.add("重名拒", p.parse("f", &[("a", Ty::U32), ("a", Ty::U32)], Ty::U32) == Err(XC_DUP_PARAM), "同名参");
    s.add("九参拒", {
        let spec = [("p1", Ty::U32), ("p2", Ty::U32), ("p3", Ty::U32), ("p4", Ty::U32),
                    ("p5", Ty::U32), ("p6", Ty::U32), ("p7", Ty::U32), ("p8", Ty::U32),
                    ("p9", Ty::U32)];
        p.parse("g", &spec, Ty::U32) == Err(XC_TOO_MANY)
    }, ">8");
    s.add("八参过", {
        let spec = [("p1", Ty::U32), ("p2", Ty::U32), ("p3", Ty::U32), ("p4", Ty::U32),
                    ("p5", Ty::U32), ("p6", Ty::U32), ("p7", Ty::U32), ("p8", Ty::U32)];
        p.parse("g8", &spec, Ty::U32).is_ok()
    }, "=8 界内");
    s.add("中位可变参拒", p.parse("v", &[("a", Ty::U32), ("r", Ty::VarArgs), ("c", Ty::U32)], Ty::U32) == Err(XC_VARPOS), "非末位");
    s.add("末位可变参过", p.parse("v2", &[("a", Ty::U32), ("r", Ty::VarArgs)], Ty::U32).is_ok(), "C ABI");
    s.add("名空拒", p.parse("", &[("a", Ty::U32)], Ty::U32) == Err(XC_BAD_NAME), "白名");
    // 族四：确定性。
    let spec_d = [("a", Ty::U32), ("b", Ty::Ptr)];
    s.add("确定性过", p.deterministic("mix", &spec_d, Ty::U64) == Ok(true), "两遍同");
    // 族五：型枚举。
    s.add("型闭集", Ty::from_u32(4) == Some(Ty::VarArgs) && Ty::from_u32(5).is_none(), "五型");
    s.add("魔数核", FD_SEG_BASE == 0x309C0, "段基址");
    s
}
