//! UNX-F36112-J1 SYNCHRONIZE 位与等待语义的挂点契约。
//! 判据：① 可等待对象类型表（12 类）× SYNCHRONIZE 需求矩阵：
//! 可等待 ⇒ 等待操作必须持 SYNCHRONIZE；不可等待类型请求
//! SYNCHRONIZE 显性拒；② 位值与 36101 标准段锚守恒；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：等待挂点契约载体——SYNCHRONIZE 位值锚定 36101
//! 标准段（位 20）；等待门供句柄面（B12）与对象管理器消费。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_BIT: u16 = SEG_BASE + 2; // SYNCHRONIZE 位值与真源锚不符
pub const E_NOSYNC: u16 = SEG_BASE + 3; // 可等待类型等待操作缺 SYNCHRONIZE
pub const E_WAITLESS: u16 = SEG_BASE + 4; // 不可等待类型请求 SYNCHRONIZE
pub const E_NOENT: u16 = SEG_BASE + 5; // 类型表外查询（显性拒）
pub const E_DUP: u16 = SEG_BASE + 6; // 类型名重复（表纪律破）
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// SYNCHRONIZE 位值（bit 20；真源锚 = 36101 标准段内）。
pub const SYNC_BIT: u32 = 0x0010_0000;

/// 类型等待项：名 + 可等待标记。
#[derive(Debug, PartialEq, Eq)]
pub struct WaitType {
    pub name: &'static str,
    pub waitable: bool,
}

/// 可等待对象类型表（12 类；waitable=true ⇒ 等待操作需 SYNCHRONIZE）。
pub const WT_N: usize = 12;
pub const WAIT_TYPES: [WaitType; WT_N] = [
    WaitType { name: "File", waitable: true },
    WaitType { name: "Event", waitable: true },
    WaitType { name: "Semaphore", waitable: true },
    WaitType { name: "Mutant", waitable: true },
    WaitType { name: "Process", waitable: true },
    WaitType { name: "Thread", waitable: true },
    WaitType { name: "Timer", waitable: true },
    WaitType { name: "Section", waitable: true },
    WaitType { name: "Token", waitable: false },
    WaitType { name: "WindowStation", waitable: false },
    WaitType { name: "Desktop", waitable: false },
    WaitType { name: "RegistryKey", waitable: false },
];

/// 按名查等待项（表外显性拒）。
pub fn lookup(name: &str) -> Result<&'static WaitType, u16> {
    let mut i = 0;
    while i < WT_N {
        if WAIT_TYPES[i].name == name {
            return Ok(&WAIT_TYPES[i]);
        }
        i += 1;
    }
    Err(E_NOENT)
}

/// 类型是否可等待。
pub fn waitable(name: &str) -> Result<bool, u16> {
    Ok(lookup(name)?.waitable)
}

/// 等待门：等待操作挂点。
/// - 可等待类型：请求掩码须含 SYNC_BIT，否则 E_NOSYNC。
/// - 不可等待类型：请求掩码含 SYNC_BIT → E_WAITLESS（显性拒）。
pub fn wait_gate(name: &str, request: u32) -> Result<(), u16> {
    let t = lookup(name)?;
    if t.waitable {
        if request & SYNC_BIT == 0 {
            return Err(E_NOSYNC);
        }
        Ok(())
    } else {
        if request & SYNC_BIT != 0 {
            return Err(E_WAITLESS);
        }
        Ok(())
    }
}

/// 挂点需求矩阵：类型名 × 操作 → 是否需要 SYNCHRONIZE。
/// 操作码：0=Wait 1=Resume 2=Signal 3=Query（只有 Wait 需同步位）。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum WaitOp {
    Wait = 0x0001,
    Resume = 0x0002,
    Signal = 0x0004,
    Query = 0x0008,
}

impl WaitOp {
    pub fn code(self) -> u16 {
        self as u16
    }
    /// 该操作是否挂 SYNCHRONIZE 需求。
    pub fn needs_sync(self) -> bool {
        matches!(self, WaitOp::Wait)
    }
}

/// 操作级门：Wait 走 wait_gate 语义；其余操作不挂同步位需求。
pub fn op_gate(name: &str, op: WaitOp, request: u32) -> Result<(), u16> {
    if op.needs_sync() {
        wait_gate(name, request)
    } else {
        // 非 Wait 操作：SYNC 位可有可无，但不可等待类型仍不许虚挂。
        let t = lookup(name)?;
        if !t.waitable && request & SYNC_BIT != 0 {
            return Err(E_WAITLESS);
        }
        Ok(())
    }
}

/// 可等待类计数（账面锚：8 类可等待 + 4 类不可等待）。
pub fn waitable_count() -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < WT_N {
        if WAIT_TYPES[i].waitable {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 表纪律自检：名唯一非空、可等待/不可等待两群齐备。
fn table_integrity() -> Result<(), u16> {
    let mut wait_n = 0;
    let mut i = 0;
    while i < WT_N {
        if WAIT_TYPES[i].name.is_empty() {
            return Err(E_DUP);
        }
        let mut j = 0;
        while j < i {
            if WAIT_TYPES[j].name == WAIT_TYPES[i].name {
                return Err(E_DUP);
            }
            j += 1;
        }
        if WAIT_TYPES[i].waitable {
            wait_n += 1;
        }
        i += 1;
    }
    if wait_n == 0 || wait_n == WT_N {
        return Err(E_DUP);
    }
    Ok(())
}

/// 演练：R1 位值锚与表纪律；R2 逐类等待门正例；R3 等待门负例；
/// R4 操作级门矩阵；R5 表外查询 + 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：位值锚——SYNC_BIT ∈ 36101::STD_USED 且单置位；表纪律。
    if VER != 1 {
        return Err(E_VER);
    }
    if SYNC_BIT != 0x0010_0000 || SYNC_BIT & !mt::STD_USED != 0 {
        return Err(E_BIT);
    }
    if mt::bit_name(20).map_err(|_| E_BIT)? != "SYNCHRONIZE" {
        return Err(E_BIT);
    }
    table_integrity()?;
    if waitable_count() != 8 {
        return Err(E_DUP);
    }
    hits += 1;
    // R2：逐类等待门——可等待 + SYNC 过；不可等待 + 无 SYNC 过。
    let mut i = 0;
    while i < WT_N {
        let t = &WAIT_TYPES[i];
        if t.waitable {
            wait_gate(t.name, SYNC_BIT)?;
            wait_gate(t.name, SYNC_BIT | 0x0000_0089)?;
        } else {
            wait_gate(t.name, 0)?;
            wait_gate(t.name, 0x0000_0089)?;
        }
        hits += 1;
        i += 1;
    }
    // R3：等待门负例——可等待缺 SYNC 必拒；不可等待虚挂必拒。
    let bads: [(&str, u32); 8] = [
        ("File", 0),
        ("Event", 0x0002_0000),
        ("Semaphore", 0x0000_0089),
        ("Process", mt::STD_USED & !SYNC_BIT),
        ("Token", SYNC_BIT),
        ("Desktop", SYNC_BIT),
        ("RegistryKey", SYNC_BIT | 0x0002_0019),
        ("WindowStation", SYNC_BIT),
    ];
    let mut i = 0;
    while i < 8 {
        let (name, req) = bads[i];
        let t = lookup(name).map_err(|_| E_NOENT)?;
        let expect = if t.waitable { E_NOSYNC } else { E_WAITLESS };
        if wait_gate(name, req) != Err(expect) {
            return Err(expect);
        }
        hits += 1;
        i += 1;
    }
    // R4：操作级门矩阵——Wait 挂需求；Resume/Signal/Query 不挂
    // （可等待类型无 SYNC 也过）。
    let ops = [WaitOp::Wait, WaitOp::Resume, WaitOp::Signal, WaitOp::Query];
    let mut i = 0;
    while i < WT_N {
        let t = &WAIT_TYPES[i];
        let mut k = 0;
        while k < 4 {
            let req = if t.waitable { SYNC_BIT } else { 0 };
            op_gate(t.name, ops[k], req)?;
            k += 1;
        }
        // 非 Wait 操作：可等待类型无 SYNC 也必须过。
        op_gate(t.name, WaitOp::Query, if t.waitable { 0 } else { 0 })?;
        hits += 1;
        i += 1;
    }
    if WaitOp::Wait.code() != 1 || WaitOp::Query.code() != 8 {
        return Err(E_NEG);
    }
    hits += 1;
    // R5：表外查询 + 负例注入（伪 waitable 表必须被检出）。
    let unknowns: [&str; 6] = ["file", "EVENT", "Key", "MutantX", "", "SymbolicLink"];
    let mut i = 0;
    while i < 6 {
        if lookup(unknowns[i]).is_ok() {
            return Err(E_NOENT);
        }
        hits += 1;
        i += 1;
    }
    struct FakeTable;
    let fake_waitable = |name: &str| -> bool { name.starts_with("File") }; // 伪表：只认 File
    let mut i = 0;
    let mut caught = 0;
    while i < WT_N {
        let t = &WAIT_TYPES[i];
        if fake_waitable(t.name) != t.waitable {
            caught += 1;
        }
        i += 1;
    }
    // 伪表只认 File：与真表的差异 = 7 个可等待非 File 类。
    if caught != 7 {
        return Err(E_NEG);
    }
    let _ = FakeTable;
    let _ = vec![0u8; 0];
    let mut ledger: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < WT_N {
        if WAIT_TYPES[i].waitable {
            ledger.push(WAIT_TYPES[i].name);
        }
        i += 1;
    }
    if ledger.len() != 8 {
        return Err(E_DUP);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(41));
    }

    #[test]
    fn waitable_needs_sync() {
        assert_eq!(wait_gate("Event", 0), Err(E_NOSYNC));
        assert!(wait_gate("Event", SYNC_BIT).is_ok());
    }

    #[test]
    fn non_waitable_rejects_sync() {
        assert_eq!(wait_gate("Token", SYNC_BIT), Err(E_WAITLESS));
        assert_eq!(wait_gate("RegistryKey", 0), Ok(()));
    }

    #[test]
    fn unknown_type_rejected() {
        assert_eq!(lookup("file"), Err(E_NOENT));
        assert_eq!(lookup(""), Err(E_NOENT));
    }
}

pub fn run_unx36112_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36112_syncbit");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a00, "0x3A00~0x3A0F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "等待门/矩阵/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("位值锚", SYNC_BIT == 0x00100000 && SYNC_BIT & mt::STD_USED == SYNC_BIT, "SYNCHRONIZE ∈ 36101::STD_USED");
    cs.add("表容量", WT_N == 12, "12 类等待表（8 可等待 + 4 不可等待）");
    cs.add("表外显性拒", lookup("X").err() == Some(E_NOENT), "未登记类型查询非静默");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_BIT => "SYNCHRONIZE 位值与真源锚不符",
        E_NOSYNC => "可等待类型等待操作缺 SYNCHRONIZE",
        E_WAITLESS => "不可等待类型请求 SYNCHRONIZE",
        E_NOENT => "类型表外查询（显性拒）",
        E_DUP => "类型名重复（表纪律破）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
