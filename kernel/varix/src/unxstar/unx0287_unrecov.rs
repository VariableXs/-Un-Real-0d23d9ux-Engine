//! unx0287_unrecov —— UNX-F0287 不可恢复检测（三重故障/栈耗尽）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0287 · 不可恢复检测 · 目标 240 行
//! 判据原文：UNX-F0287-J1 两类不可恢复态判定：三重故障（#DF，
//!           双重故障的处理程序自己又出错）与栈耗尽（栈哨兵带
//!           破）——两者都是「处理机制本身失效」，必须走独立
//!           最小路径（不可再依赖正常栈/正常处理程序），判定
//!           后进入停机账（不可恢复 = 不许假装可恢复）。
//! 组件：故障分类（三重/栈耗尽/可恢复三类）+ 栈哨兵（金丝雀
//!       值 + 带破核）+ 停机账（原因 + 现场指纹 + 只读冻结）。
//! 职责定位：不可恢复检测是「错误处理的错误处理」——#DF 意味
//!   着异常处理链已经自我崩坏，此时唯一的正确动作是把现场
//!   记下来然后停机；任何「再试一次」的乐观都会把账面变成
//!   不可信。停机账只读冻结：死后不许再改口。
//! 性能逐项分解：分类 O(1)；哨兵核 O(1)；停机账 O(1)。
//! 诊断码段：0x20580（0x20581~0x20589 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const UR_VERSION: &str = "unx0287_unrecov-v1";

/// 诊断段基址（0x20580 段 · 本模块独占）。
pub const UR_SEG_BASE: u32 = 0x20580;
pub const XC_DF_TRIPLE: u32 = 0x20581; // 三重故障
pub const XC_STACK_CANARY: u32 = 0x20582; // 栈哨兵带破
pub const XC_ALREADY_HALTED: u32 = 0x20583; // 已停机再记账
pub const XC_CLASS_OOR: u32 = 0x20584; // 故障类越域
pub const XC_FINGER_ZERO: u32 = 0x20585; // 现场指纹零
pub const XC_RESUME_DENY: u32 = 0x20586; // 停机后请求续行
pub const XC_CANARY_UNSET: u32 = 0x20587; // 哨兵未布
pub const XC_LEDGER_FULL: u32 = 0x20588; // 停机账满
pub const XC_VECTOR_OOR: u32 = 0x20589; // 向量号越域

/// 故障三类。
pub const F_RECOVERABLE: u8 = 0; // 可恢复（正常处理路径）
pub const F_DF_DOUBLE: u8 = 1; // 双重故障（#DF 向量 8）
pub const F_STACK_EXHAUST: u8 = 2; // 栈耗尽（哨兵带破）

/// 栈哨兵金丝雀值。
pub const CANARY: u64 = 0xCAFE_BABE_DEAD_F00D;

/// #DF 向量号。
pub const VEC_DF: u32 = 8;

/// 故障分类核：向量号 + 哨兵态 → 分类。
pub fn classify(vector: u32, canary_ok: bool, canary_set: bool) -> Result<u8, u32> {
    if vector > 31 {
        return Err(XC_VECTOR_OOR);
    }
    if !canary_set {
        return Err(XC_CANARY_UNSET); // 哨兵没布 = 无从判栈
    }
    if vector == VEC_DF {
        return Ok(F_DF_DOUBLE);
    }
    if !canary_ok {
        return Ok(F_STACK_EXHAUST);
    }
    Ok(F_RECOVERABLE)
}

/// 现场指纹：FNV(向量 || 栈指针 || 错误码)。
pub fn fingerprint(vector: u32, rsp: u64, err: u32) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for v in [vector as u64, rsp, err as u64].iter() {
        for shift in 0..8 {
            h ^= (v >> (shift * 8)) as u8 as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 停机账（只读冻结）。
pub struct HaltLedger {
    entries: Vec<(u8, u64)>, // (故障类, 现场指纹)
    halted: bool,
}

impl HaltLedger {
    pub fn new() -> HaltLedger {
        HaltLedger { entries: Vec::new(), halted: false }
    }

    /// 立案停机：分类必须是不可恢复两类（可恢复不许进停机账）。
    pub fn halt(&mut self, class: u8, fp: u64) -> Result<(), u32> {
        if self.halted {
            return Err(XC_ALREADY_HALTED);
        }
        if class != F_DF_DOUBLE && class != F_STACK_EXHAUST {
            return Err(XC_CLASS_OOR);
        }
        if fp == 0 {
            return Err(XC_FINGER_ZERO);
        }
        if self.entries.len() >= 16 {
            return Err(XC_LEDGER_FULL);
        }
        self.entries.push((class, fp));
        self.halted = true; // 第一条即停机
        Ok(())
    }

    /// 停机态。
    pub fn halted(&self) -> bool {
        self.halted
    }

    /// 停机账（只读）。
    pub fn entries(&self) -> &[(u8, u64)] {
        &self.entries
    }

    /// 续行请求核：停机后一律拒（不可恢复 = 不许假装可恢复）。
    pub fn resume_request(&self) -> Result<(), u32> {
        if self.halted {
            return Err(XC_RESUME_DENY);
        }
        Ok(())
    }
}

/// 栈哨兵：布置与核验。
pub struct StackSentinel {
    base: u64,
    planted: bool,
}

impl StackSentinel {
    pub fn plant(base: u64) -> StackSentinel {
        StackSentinel { base, planted: true }
    }

    pub fn planted(&self) -> bool {
        self.planted
    }

    /// 核验：当前值与金丝雀比对。
    pub fn check(&self, current: u64) -> bool {
        self.planted && current == CANARY
    }

    /// 哨兵基址。
    pub fn base(&self) -> u64 {
        self.base
    }
}

pub fn run_unx0287_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0287_unrecov");
    // 族一：故障分类。
    cs.add("#DF 分类", classify(VEC_DF, true, true) == Ok(F_DF_DOUBLE), "向量 8");
    cs.add("栈破分类", classify(3, false, true) == Ok(F_STACK_EXHAUST), "哨兵带破");
    cs.add("可恢复分类", classify(3, true, true) == Ok(F_RECOVERABLE), "哨兵完好");
    cs.add("哨兵未布拒", classify(3, true, false).is_err(), "CANARY_UNSET");
    cs.add("向量越域拒", classify(32, true, true).is_err(), "VECTOR_OOR");
    // 族二：停机账。
    let fp1 = fingerprint(VEC_DF, 0x7F00_0000, 0);
    let fp2 = fingerprint(14, 0x7EFF_FFF8, 0x2);
    cs.add("指纹非零", fp1 != 0 && fp2 != 0, "现场入签");
    cs.add("指纹区分", fp1 != fp2, "不同现场不同签");
    let mut ledger = HaltLedger::new();
    cs.add("三重停机", ledger.halt(F_DF_DOUBLE, fp1).is_ok(), "#DF 入账");
    cs.add("停机态", ledger.halted(), "第一案即停");
    cs.add("续行拒", ledger.resume_request() == Err(XC_RESUME_DENY), "不许假装可恢复");
    cs.add("再案拒", ledger.halt(F_STACK_EXHAUST, fp2) == Err(XC_ALREADY_HALTED), "已停");
    // 族三：可恢复不许进停机账。
    let mut l2 = HaltLedger::new();
    cs.add("可恢复拒停", l2.halt(F_RECOVERABLE, fp1) == Err(XC_CLASS_OOR), "分类闸");
    cs.add("未停可续行", l2.resume_request().is_ok(), "正常态放行");
    // 族四：栈哨兵。
    let s = StackSentinel::plant(0x7F00_0000);
    cs.add("哨兵已布", s.planted(), "金丝雀就位");
    cs.add("完好核", s.check(CANARY), "值等于金丝雀");
    cs.add("带破核", !s.check(CANARY ^ 1), "一字节破即检出");
    cs.add("基址记账", s.base() == 0x7F00_0000, "现场基址");
    // 族五：分类闭环（分类结果直接驱动停机决策）。
    let c = classify(VEC_DF, false, true);
    cs.add("#DF 优先", c == Ok(F_DF_DOUBLE), "向量分类压过哨兵");
    let mut l3 = HaltLedger::new();
    let ok = match classify(VEC_DF, true, true) {
        Ok(k) => l3.halt(k, fp1).is_ok(),
        Err(_) => false,
    };
    cs.add("分类驱动停机", ok && l3.halted(), "分类→停机链");
    cs.add("账只读", ledger.entries().len() == 1, "冻结单案");
    cs
}
