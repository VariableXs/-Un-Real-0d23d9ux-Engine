//! unx0425_pfecode —— UNX-F0425 异常错误码账（#PF 错误码解析）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0425 · 异常错误码账 · 目标 240 行
//! 判据原文：UNX-F0425-J1 #PF 错误码逐位解析（位 0 = P 存在位，
//!           位 1 = W/R 写位，位 2 = U/S 用户位，位 3 = RSVD 保留
//!           位，位 4 = I/D 取指位）；解析出 (原因, 访问型, 特
//!           权级) 三元组； cause 分四类：缺页(P=0)/保护违例
//!           (P=1)/保留位违例(RSVD=1)/指令取指(I/D=1)；解析可
//!           重放（同码同三元组）；非法组合立案（RSVD 且非 P）。
//! 组件：错误码位域（u32 位提取）+ 解析器（位 → 三元组）+
//!       原因分类账（四类枚举）+ 重放核 + 非法组合闸。
//! 职责定位：#PF 错误码是「缺页的案卷首页」——五个位就是五条
//!   线索：P 告诉你页在不在，W 告诉你是读是写，U 告诉你是内
//!   核是用户，RSVD 告诉你页表有没有被写花，I/D 告诉你是不是
//!   在取指。解析成三元组是给后续处理（分配/杀进程/panic）分
//!   流的依据；解析错了分流就错，所以重放核是硬要求。
//! 性能逐项分解：解析 O(1)；分类 O(1)；重放 O(1)。
//! 诊断码段：0x21340（0x21341~0x21349 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PE_VERSION: &str = "unx0425_pfecode-v1";

/// 诊断段基址（0x21340 段 · 本模块独占）。
pub const PE_SEG_BASE: u32 = 0x21340;
pub const XC_ILLEGAL_COMBO: u32 = 0x21341; // 非法组合（RSVD 且非 P）
pub const XC_CAUSE_OOR: u32 = 0x21342; // 原因越域
pub const XC_ACCESS_OOR: u32 = 0x21343; // 访问型越域
pub const XC_REPLAY: u32 = 0x21344; // 重放不一致
pub const XC_MAGIC: u32 = 0x21345; // 账魔数破
pub const XC_ADDR_MISS: u32 = 0x21346; // 故障地址未附
pub const XC_DUP_CASE: u32 = 0x21347; // 案重号
pub const XC_CLASS_LOST: u32 = 0x21348; // 分类丢失
pub const XC_BIT_OOR: u32 = 0x21349; // 位提取越域

/// 位掩码。
pub const BIT_P: u32 = 1;       // 位 0：存在
pub const BIT_W: u32 = 1 << 1;  // 位 1：写
pub const BIT_U: u32 = 1 << 2;  // 位 2：用户
pub const BIT_R: u32 = 1 << 3;  // 位 3：保留位违例
pub const BIT_I: u32 = 1 << 4;  // 位 4：取指

/// 原因四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCause {
    /// 缺页（P=0）。
    NotPresent = 1,
    /// 保护违例（P=1，无 RSVD/I/D）。
    Protect = 2,
    /// 保留位违例（RSVD=1）。
    ReservedBit = 3,
    /// 指令取指（I/D=1）。
    InstrFetch = 4,
}

/// 访问型：1 = 读 2 = 写 3 = 取指。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccessKind {
    Read = 1,
    Write = 2,
    Fetch = 3,
}

/// 特权级：0 = 内核 3 = 用户。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priv {
    Kernel = 0,
    User = 3,
}

/// 解析三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PfTriple {
    pub cause: PfCause,
    pub access: AccessKind,
    pub priv_: Priv,
}

/// 解析器：错误码 → 三元组；RSVD 且非 P = 非法组合拒。
pub fn parse_pf(code: u32) -> Result<PfTriple, u32> {
    let p = code & BIT_P != 0;
    let w = code & BIT_W != 0;
    let u = code & BIT_U != 0;
    let r = code & BIT_R != 0;
    let i = code & BIT_I != 0;
    if r && !p {
        return Err(XC_ILLEGAL_COMBO); // 不存在的页谈不上保留位违例
    }
    let cause = if r {
        PfCause::ReservedBit
    } else if i {
        PfCause::InstrFetch
    } else if !p {
        PfCause::NotPresent
    } else {
        PfCause::Protect
    };
    let access = if i {
        AccessKind::Fetch
    } else if w {
        AccessKind::Write
    } else {
        AccessKind::Read
    };
    let priv_ = if u { Priv::User } else { Priv::Kernel };
    Ok(PfTriple { cause, access, priv_ })
}

/// 位提取核：指定位值。
pub fn bit_of(code: u32, bit: u32) -> Result<bool, u32> {
    if bit > 31 {
        return Err(XC_BIT_OOR);
    }
    Ok(code & (1 << bit) != 0)
}

/// 分类账。
pub struct PfBook {
    cases: Vec<(u32, PfTriple)>,
}

impl PfBook {
    /// 建空账。
    pub fn new() -> PfBook {
        PfBook { cases: Vec::new() }
    }

    /// 立案：错误码 + 故障地址；重号拒、非法组合拒。
    pub fn record(&mut self, code: u32, addr: u64) -> Result<(), u32> {
        if addr == 0 {
            return Err(XC_ADDR_MISS);
        }
        let t = parse_pf(code)?;
        // 重号：同码同三元组视为重案。
        for (c, t0) in self.cases.iter() {
            if *c == code && t0 == &t {
                return Err(XC_DUP_CASE);
            }
        }
        self.cases.push((code, t));
        Ok(())
    }

    /// 案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 分类统计：(缺页, 保护, 保留位, 取指)。
    pub fn class_counts(&self) -> (u32, u32, u32, u32) {
        let mut c = (0u32, 0u32, 0u32, 0u32);
        for (_, t) in self.cases.iter() {
            match t.cause {
                PfCause::NotPresent => c.0 += 1,
                PfCause::Protect => c.1 += 1,
                PfCause::ReservedBit => c.2 += 1,
                PfCause::InstrFetch => c.3 += 1,
            }
        }
        c
    }

    /// 分类账平：四类之和 = 案数。
    pub fn class_ok(&self) -> Result<(), u32> {
        let c = self.class_counts();
        if c.0 + c.1 + c.2 + c.3 != self.cases.len() as u32 {
            return Err(XC_CLASS_LOST);
        }
        Ok(())
    }
}

/// 重放核：同码重解同三元组。
pub fn replay_ok(code: u32) -> Result<(), u32> {
    let a = parse_pf(code)?;
    let b = parse_pf(code)?;
    if a != b {
        return Err(XC_REPLAY);
    }
    Ok(())
}

pub fn run_unx0425_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0425_pfecode");
    // 族一：四类原因解析。
    let t1 = parse_pf(0); // 全零：内核读缺页
    cs.add("缺页三元组",
        t1 == Ok(PfTriple { cause: PfCause::NotPresent, access: AccessKind::Read, priv_: Priv::Kernel }),
        "P=0 W=0 U=0");
    let t2 = parse_pf(BIT_P); // P=1：保护违例
    cs.add("保护违例",
        t2 == Ok(PfTriple { cause: PfCause::Protect, access: AccessKind::Read, priv_: Priv::Kernel }),
        "P=1 其余 0");
    let t3 = parse_pf(BIT_P | BIT_W | BIT_U); // 用户写保护违例
    cs.add("用户写保护",
        t3 == Ok(PfTriple { cause: PfCause::Protect, access: AccessKind::Write, priv_: Priv::User }),
        "P W U 全 1");
    let t4 = parse_pf(BIT_P | BIT_R); // 保留位违例
    cs.add("保留位违例",
        t4 == Ok(PfTriple { cause: PfCause::ReservedBit, access: AccessKind::Read, priv_: Priv::Kernel }),
        "RSVD=1");
    let t5 = parse_pf(BIT_P | BIT_I | BIT_U); // 用户态取指违例
    cs.add("取指违例",
        t5 == Ok(PfTriple { cause: PfCause::InstrFetch, access: AccessKind::Fetch, priv_: Priv::User }),
        "I/D=1 U=1");
    let t6 = parse_pf(BIT_P | BIT_U); // 用户读保护
    cs.add("用户读保护",
        t6 == Ok(PfTriple { cause: PfCause::Protect, access: AccessKind::Read, priv_: Priv::User }),
        "P U");
    // 族二：非法组合（RSVD 且非 P）。
    cs.add("RSVD 非 P 拒", parse_pf(BIT_R) == Err(XC_ILLEGAL_COMBO), "位 3 无位 0");
    // 族三：位提取核。
    cs.add("位 0 提取", bit_of(0b1, 0) == Ok(true), "P 位");
    cs.add("位 4 提取", bit_of(0b1_0000, 4) == Ok(true), "I/D 位");
    cs.add("位 5 提取", bit_of(0b1_0000, 5) == Ok(false), "未置位");
    cs.add("位越域拒", bit_of(1, 32) == Err(XC_BIT_OOR), "bit=32");
    // 族四：重放核。
    for code in [0u32, BIT_P, BIT_P | BIT_W | BIT_U, BIT_P | BIT_R, BIT_P | BIT_I].iter() {
        cs.add("重放稳", replay_ok(*code).is_ok(), "同码同三元组");
    }
    // 族五：分类账。
    let mut b = PfBook::new();
    let _ = b.record(0, 0x1000);
    let _ = b.record(BIT_P, 0x2000);
    let _ = b.record(BIT_P | BIT_W | BIT_U, 0x3000);
    let _ = b.record(BIT_P | BIT_R, 0x4000);
    let _ = b.record(BIT_P | BIT_I | BIT_U, 0x5000);
    cs.add("五案入库", b.len() == 5, "逐案");
    cs.add("分类 1/2/1/1", b.class_counts() == (1, 2, 1, 1), "缺页1 保护2 保留1 取指1");
    cs.add("分类账平", b.class_ok().is_ok(), "4+2=5 化简前核");
    // 族六：立案拒绝。
    let mut z = PfBook::new();
    cs.add("零地址拒", z.record(0, 0) == Err(XC_ADDR_MISS), "addr=0");
    cs.add("非法组合拒", z.record(BIT_R, 0x1000) == Err(XC_ILLEGAL_COMBO), "RSVD 非 P");
    cs
}
