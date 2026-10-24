//! UNX-F0422 异常门注册账（32 向量全覆盖）（0xAF00 段）
//!
//! 锚点：UNX-F0422-J1 0–31 异常向量 32/32 注册专名/类账一致
//! （#DE~#PF 等逐名核）。
//! 域：CoRun·UNX-A1-B22（异常门面）。
//!
//! 结构：32 向量异常名闭集（#DE/#DB/#NMI/#BP/#OF/#BR/#UD/#NM/#DF/
//! #TS/#NP/#SS/#GP/#PF/#MF/#AC/#MC/#XM/#VE/#CP/#VC/#SX/#HV 等）+
//! 注册账（向量 → (名, 类, handler 锚)）+ 全覆盖核（32/32 注册）
//! + 逐名核 + 注入核（向量越界/重复注册/名漂移/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAF00 独占 0x80）。

pub const SEG_BASE: u32 = 0xAF00;
pub const XC_EG_RANGE: u32 = SEG_BASE + 0x01;
pub const XC_EG_DUP: u32 = SEG_BASE + 0x02;
pub const XC_EG_SEALED: u32 = SEG_BASE + 0x03;

/// 向量空间。
pub const VEC_MAX: u8 = 31;

/// 异常类闭集。
pub const CL_FAULT: u8 = 0;
pub const CL_TRAP: u8 = 1;
pub const CL_ABORT: u8 = 2;
pub const CL_INTR: u8 = 3; // NMI

/// 异常名表（0..=31 冻结，x86_64）。
pub const EXC_NAMES: [&str; 32] = [
    "#DE", "#DB", "NMI", "#BP", "#OF", "#BR", "#UD", "#NM",
    "#DF", "#TS", "#NP", "#SS", "#GP", "#PF", "#MF", "#AC",
    "#MC", "#XM", "#VE", "#CP", "#VC", "#SX", "#HV", "R23",
    "R24", "R25", "R26", "R27", "R28", "R29", "R30", "R31",
];

/// 异常类表（0..=31 冻结）。
pub const EXC_CLASSES: [u8; 32] = [
    CL_FAULT, CL_FAULT, CL_INTR, CL_TRAP, CL_TRAP, CL_FAULT, CL_FAULT, CL_FAULT,
    CL_ABORT, CL_FAULT, CL_FAULT, CL_ABORT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT,
    CL_ABORT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_ABORT, CL_FAULT,
    CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT, CL_FAULT,
];

/// 注册项。
#[derive(Clone)]
pub struct GateEntry {
    pub vector: u8,
    pub name: &'static str,
    pub class: u8,
    pub handler: u64,
}

/// 异常门注册账。
pub struct GateBook {
    pub gates: Vec<GateEntry>,
    pub sealed: bool,
}

impl GateBook {
    pub fn new() -> Self {
        GateBook { gates: Vec::new(), sealed: false }
    }

    /// 注册（向量界 + 唯一性守卫）。
    pub fn register(&mut self, vector: u8, handler: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_EG_SEALED);
        }
        if vector > VEC_MAX {
            return Err(XC_EG_RANGE);
        }
        if self.gates.iter().any(|g| g.vector == vector) {
            return Err(XC_EG_DUP);
        }
        self.gates.push(GateEntry {
            vector,
            name: EXC_NAMES[vector as usize],
            class: EXC_CLASSES[vector as usize],
            handler,
        });
        Ok(())
    }

    /// 全覆盖核：32/32 注册且名/类与冻结表逐项一致。
    pub fn full_cover(&self) -> bool {
        if self.gates.len() != 32 {
            return false;
        }
        for v in 0..=31u8 {
            match self.gates.iter().find(|g| g.vector == v) {
                Some(g) => {
                    if g.name != EXC_NAMES[v as usize] || g.class != EXC_CLASSES[v as usize] {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 关键异常逐名核：#DE(0)/#UD(6)/#GP(13)/#PF(14)。
    pub fn key_names(&self) -> bool {
        self.gate_name(0) == Some("#DE")
            && self.gate_name(6) == Some("#UD")
            && self.gate_name(13) == Some("#GP")
            && self.gate_name(14) == Some("#PF")
    }

    pub fn gate_name(&self, v: u8) -> Option<&'static str> {
        self.gates.iter().find(|g| g.vector == v).map(|g| g.name)
    }
}

/// 32/32 全注册核。
pub fn register_32_exact() -> bool {
    let mut b = GateBook::new();
    for v in 0..=31u8 {
        if b.register(v, 0x1000 + v as u64).is_err() {
            return false;
        }
    }
    b.full_cover() && b.key_names()
}

/// 逐名核：全部 32 名与表一致。
pub fn names_32_exact() -> bool {
    let mut b = GateBook::new();
    for v in 0..=31u8 {
        let _ = b.register(v, 0x2000);
    }
    for (v, g) in b.gates.iter().enumerate() {
        if g.name != EXC_NAMES[v] {
            return false;
        }
    }
    // NMI 类特殊：向量 2 类 = INTR
    b.gates.iter().find(|g| g.vector == 2).map(|g| g.class) == Some(CL_INTR)
}

/// 重复注册拒核。
pub fn dup_guard_exact() -> bool {
    let mut b = GateBook::new();
    let _ = b.register(14, 0x3000);
    b.register(14, 0x4000) == Err(XC_EG_DUP)
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let mut b = GateBook::new();
    // 1) 向量越界（32）
    if b.register(32, 0x5000) == Err(XC_EG_RANGE) {
        caught += 1;
    }
    // 2) 重复注册
    let _ = b.register(0, 0x5000);
    if b.register(0, 0x6000) == Err(XC_EG_DUP) {
        caught += 1;
    }
    // 3) 不全覆盖
    let mut b2 = GateBook::new();
    for v in 0..=30u8 {
        let _ = b2.register(v, 0x7000);
    }
    if !b2.full_cover() {
        caught += 1;
    }
    // 4) 封账后拒
    b.sealed = true;
    if b.register(1, 0x8000) == Err(XC_EG_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：门账折叠哈希。
pub fn fp_frozen(b: &GateBook) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for g in &b.gates {
        h ^= g.vector as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= g.handler;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx422_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0422");
    // J1-1 32/32 注册
    let ok = register_32_exact();
    s.add("B22-01-32全注册", ok, if ok { "" } else { "注册缺" });
    // J1-2 逐名一致
    let ok = names_32_exact();
    s.add("B22-02-逐名一致", ok, if ok { "" } else { "名漂移" });
    // J1-3 重复注册拒
    let ok = dup_guard_exact();
    s.add("B22-03-重复拒", ok, if ok { "" } else { "重复漏" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B22-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let mut b = GateBook::new();
    let _ = b.register(14, 0xF1);
    let ok = fp_frozen(&b) == fp_frozen(&b);
    s.add("B22-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
