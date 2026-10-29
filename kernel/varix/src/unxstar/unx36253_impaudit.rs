//! UNX-F36253-J1 模拟审计事件（谁模拟成谁/何时/做什么）。
//! 判据：① 模拟事件 100% 落账 100 例（三要素齐备）；② 漏报注入
//! 10 例被对账器检出；③ 纯功能 ≥300 行。
//! 设计纪律：模拟审计的三要素是「谁模拟成谁（源/目标令牌）、
//! 何时（序号时钟）、做什么（操作面）」——三缺一即事件不可追责。
//! 100% 落账的纪律是对账器的**完备性**：账内事件数 = 模拟事件数，
//! 漏报（事件发生但账里没有）由对账器以「账外序号空洞」检出。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xA3B0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_MISS: u16 = SEG_BASE + 2; // 漏报（账外事件）
pub const E_PARTIAL: u16 = SEG_BASE + 3; // 三要素缺项
pub const E_SEQ: u16 = SEG_BASE + 4; // 序号时钟空洞
pub const E_BAL: u16 = SEG_BASE + 5; // 对账不闭合
pub const E_ILLEGAL: u16 = SEG_BASE + 6; // 越合法区

/// 模块版本锚。
pub const VER: u32 = 1;

/// 审计事件（三要素冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ImpEvent {
    /// 源令牌指纹（谁发起模拟）。
    pub from_fp: u32,
    /// 目标令牌指纹（模拟成谁）。
    pub to_fp: u32,
    /// 序号时钟（何时——单调递增）。
    pub seq: u32,
    /// 操作面（做什么——开始/结束/查询三面）。
    pub action: u8,
}

pub const ACT_BEGIN: u8 = 0;
pub const ACT_END: u8 = 1;
pub const ACT_QUERY: u8 = 2;

impl ImpEvent {
    /// 三要素齐备（源/目标/序号非零 + 操作面合法）。
    pub fn complete(&self) -> bool {
        self.from_fp != 0 && self.to_fp != 0 && self.seq != 0 && self.action <= ACT_QUERY
    }
}

/// 审计账（append-only + 序号时钟）。
pub struct AuditLedger {
    entries: alloc::vec::Vec<ImpEvent>,
    clock: u32,
}

const CAP: usize = 128;

impl AuditLedger {
    pub fn new() -> Self {
        AuditLedger { entries: alloc::vec::Vec::new(), clock: 0 }
    }

    /// 落账（三要素自动补齐 + 时钟推进）。
    pub fn record(&mut self, from_fp: u32, to_fp: u32, action: u8) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if from_fp == 0 || to_fp == 0 || action > ACT_QUERY {
            return Err(E_PARTIAL);
        }
        if self.entries.len() >= CAP {
            return Err(E_BAL);
        }
        self.clock = self.clock.wrapping_add(1);
        self.entries.push(ImpEvent { from_fp, to_fp, seq: self.clock, action });
        Ok(())
    }

    /// 事件数（对账面）。
    pub fn count(&self) -> u32 {
        self.entries.len() as u32
    }

    /// 时钟（事件发生的总次数真源——漏报检测的基准）。
    pub fn clock_now(&self) -> u32 {
        self.clock
    }

    /// 完备性对账：账内数 == 时钟数 且 序号无空洞（判据 ② 落点）。
    pub fn reconcile(&self) -> Result<(), u16> {
        if self.entries.len() as u32 != self.clock {
            return Err(E_MISS); // 有事件发生但未落账
        }
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].seq != (i as u32) + 1 {
                return Err(E_SEQ);
            }
            if !self.entries[i].complete() {
                return Err(E_PARTIAL);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 模拟事件 100% 落账 100 例（判据 ① 落点：开始/结束/查询三面
/// 全落账，终态对账闭合）。
pub fn audit_hundred() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = AuditLedger::new();
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let from = (r & 0xFFFF) | 1;
        let to = (r.rotate_left(8) & 0xFFFF) | 1;
        let action = (i % 3) as u8;
        lg.record(from, to, action)?;
        i += 1;
    }
    if lg.count() != 100 || lg.clock_now() != 100 {
        return Err(E_MISS);
    }
    lg.reconcile()?;
    Ok(100)
}

/// 漏报注入 10 例被对账器检出（判据 ② 落点：构造「时钟已推进但
/// 账未记」的漏报态——对账器必须以 E_MISS 检出每一次）。
pub fn miss_ten_detected() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut detected = 0u32;
    let mut a = 0u32;
    while a < 10 {
        let r = a.wrapping_mul(2_654_435_761).wrapping_add(0x5EED);
        let mut lg = AuditLedger::new();
        // 正常落 3 笔。
        let mut k = 0;
        while k < 3 {
            lg.record((r & 0xFFFF) | 1, (r.rotate_left(5) & 0xFFFF) | 1, (k % 3) as u8).map_err(|_| E_PARTIAL)?;
            k += 1;
        }
        // 注入漏报：时钟多推进 1..=3 次而账未记（模拟事件发生但
        // 审计写入被旁路）。
        let miss = (a % 3) + 1;
        let mut m = 0;
        while m < miss {
            lg.clock = lg.clock.wrapping_add(1); // 旁路 record
            m += 1;
        }
        // 对账器必须检出。
        if lg.reconcile().is_err() {
            detected += 1;
        } else {
            return Err(E_BAL); // 漏报未检出 = 审计失灵
        }
        a += 1;
    }
    Ok(detected)
}

/// 按操作面查询（审计账的读口：开始/结束/查询三类各自可检索——
/// 追责面的第二形态：不只可按序读，还可按语义过滤）。
pub fn query_by_action(lg: &AuditLedger, action: u8) -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if action > ACT_QUERY {
        return Err(E_PARTIAL);
    }
    let mut n = 0u32;
    let mut i = 0;
    while i < lg.entries.len() {
        if lg.entries[i].action == action {
            n += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 三面分类账直证（100 例三类各 ~33/34，分类和 = 总数——账面
/// 分解守恒）。
pub fn action_breakdown() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = AuditLedger::new();
    let mut i = 0u32;
    while i < 100 {
        let r = i.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        lg.record((r & 0xFFFF) | 1, (r.rotate_left(8) & 0xFFFF) | 1, (i % 3) as u8)?;
        i += 1;
    }
    let b = query_by_action(&lg, ACT_BEGIN)?;
    let e = query_by_action(&lg, ACT_END)?;
    let q = query_by_action(&lg, ACT_QUERY)?;
    if b.wrapping_add(e).wrapping_add(q) != 100 {
        return Err(E_BAL);
    }
    // 非法操作面查询：拒。
    if query_by_action(&lg, 9).is_ok() {
        return Err(E_PARTIAL);
    }
    lg.reconcile()?;
    Ok(b)
}

/// 审计指纹（账折叠——同账恒同指纹，防漂锚）。
pub fn audit_fingerprint(lg: &AuditLedger) -> u32 {
    let mut h: u32 = 0x4A17_0000;
    let mut i = 0;
    while i < lg.entries.len() {
        let e = lg.entries[i];
        h = h.wrapping_mul(31).wrapping_add(e.from_fp);
        h = h.wrapping_mul(31).wrapping_add(e.to_fp);
        h = h.wrapping_mul(31).wrapping_add(e.seq);
        i += 1;
    }
    h
}

/// 审计容量语义（CAP 满册显性拒——不静默覆盖不丢账）。
pub fn ledger_cap_semantics() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut lg = AuditLedger::new();
    let mut i = 0;
    while i < CAP {
        lg.record(0x100 + i as u32, 0x200 + i as u32, (i % 3) as u8)?;
        i += 1;
    }
    if lg.count() != CAP as u32 {
        return Err(E_BAL);
    }
    // 满册再收：显性拒。
    if lg.record(1, 2, ACT_BEGIN).is_ok() {
        return Err(E_BAL);
    }
    lg.reconcile()
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let n = audit_hundred()?;
    let d = miss_ten_detected()?;
    // 三面分类账（按语义过滤读口）。
    action_breakdown()?;
    // 审计指纹稳定 + 容量语义。
    let mut probe = AuditLedger::new();
    probe.record(0x11, 0x22, ACT_BEGIN)?;
    let f1 = audit_fingerprint(&probe);
    let f2 = audit_fingerprint(&probe);
    if f1 != f2 {
        return Err(E_SEQ);
    }
    ledger_cap_semantics()?;
    if n != 100 || d != 10 {
        return Err(E_MISS);
    }
    Ok(n.wrapping_mul(1000).wrapping_add(d))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_MISS => "漏报（账外事件）",
        E_PARTIAL => "三要素缺项",
        E_SEQ => "序号时钟空洞",
        E_BAL => "对账不闭合",
        E_ILLEGAL => "越合法区",
        _ => "未知诊断码",
    }
}

pub fn run_unx36253_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36253_impaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0xA3B0, "0xA3B0~0xA3BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例三要素全落账/漏报注入 10 例全检出"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三要素锚", ACT_BEGIN == 0 && ACT_END == 1 && ACT_QUERY == 2, "开始/结束/查询三操作面闭集");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_partial_rejected() {
        let mut lg = AuditLedger::new();
        // 零源指纹：拒（三要素缺项）。
        assert!(lg.record(0, 5, ACT_BEGIN).err() == Some(E_PARTIAL));
        // 非法操作面：拒。
        assert!(lg.record(3, 5, 9).err() == Some(E_PARTIAL));
        assert_eq!(lg.count(), 0);
    }

    #[test]
    fn test_miss_detected() {
        assert_eq!(miss_ten_detected().ok(), Some(10));
        assert_eq!(audit_hundred().ok(), Some(100));
    }
}
