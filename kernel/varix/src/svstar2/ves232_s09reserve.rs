//! VE-F3978 · S 域演进预留（组级）
//!
//! S 域组级演进预留声明：预留面四口（新辅助技术接入位/新色标档位/新心流
//! 指标/新输入形态）。预留纪律：预留不预写——只留接口与断言位。
//! 错误路径：预写越界→回退；预留位被占→迁移评审；登记缺→补登。
//! 性能：登记 O(1)；评审 O(口)；断言 O(位)。
//! 跨批对接：八组扩展条目归集；F3998 域级预留上游；T 域衔接。
//! 无障碍与隐私：四口均无障碍向预留（域本色）；预留不引入新隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3680）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3680;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9RCode(pub u16);

impl S9RCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const PREWRITE: Self = Self(SEG_BASE + 1);
    pub const SLOT_OCCUPIED: Self = Self(SEG_BASE + 2);
    pub const REG_MISSING: Self = Self(SEG_BASE + 3);
    pub const PORT_OVER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::PREWRITE => "预写越界→回退",
            Self::SLOT_OCCUPIED => "预留位被占→迁移评审",
            Self::REG_MISSING => "登记缺→补登",
            Self::PORT_OVER => "口越界→拒收",
            _ => "未知",
        }
    }
}

// ── 预留面四口 ───────────────────────────────────────────────────────

/// 四口。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservePort {
    /// 新辅助技术接入位。
    NewAidTech,
    /// 新色标档位。
    NewColorTier,
    /// 新心流指标。
    NewFlowMetric,
    /// 新输入形态。
    NewInputForm,
}

pub const PORTS: [ReservePort; 4] = [
    ReservePort::NewAidTech,
    ReservePort::NewColorTier,
    ReservePort::NewFlowMetric,
    ReservePort::NewInputForm,
];

/// 接口位 + 断言位（只留位不预写）。
#[derive(Debug, Clone)]
pub struct PortSlot {
    pub port: ReservePort,
    /// 接口位已留。
    pub iface: bool,
    /// 断言位已留。
    pub assertion: bool,
    /// 是否已被占用（占位 → 迁移评审）。
    pub occupied: bool,
}

impl PortSlot {
    /// 只留位断言：未占用且两齐 = 合法预留。
    pub fn legal_reserve(&self) -> bool {
        self.iface && self.assertion && !self.occupied
    }
}

/// 预留册。
#[derive(Debug, Clone)]
pub struct ReserveBook {
    pub slots: Vec<PortSlot>,
}

impl ReserveBook {
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// 留位：只登记接口与断言位。
    pub fn reserve(&mut self, port: ReservePort) -> S9RCode {
        if self.slots.iter().any(|s| s.port == port) {
            return S9RCode::OK;
        }
        self.slots.push(PortSlot { port, iface: true, assertion: true, occupied: false });
        S9RCode::OK
    }

    /// 登记缺检出。
    pub fn missing(&self) -> Vec<ReservePort> {
        PORTS.iter().filter(|p| !self.slots.iter().any(|s| s.port == **p)).map(|p| *p).collect()
    }

    /// 占用：外部实现想占位 → 触发迁移评审。
    pub fn occupy(&mut self, port: ReservePort) -> S9RCode {
        match self.slots.iter_mut().find(|s| s.port == port) {
            Some(s) if !s.occupied => {
                s.occupied = true;
                S9RCode::SLOT_OCCUPIED
            }
            Some(_) => S9RCode::SLOT_OCCUPIED,
            None => S9RCode::REG_MISSING,
        }
    }

    /// 迁移评审：占位后重开新位。
    pub fn migrate(&mut self, from: ReservePort, to: ReservePort) -> S9RCode {
        if self.slots.iter().any(|s| s.port == to) {
            return S9RCode::SLOT_OCCUPIED;
        }
        match self.slots.iter_mut().find(|s| s.port == from) {
            Some(s) => {
                s.port = to;
                s.occupied = false;
                S9RCode::OK
            }
            None => S9RCode::REG_MISSING,
        }
    }

    /// 预写越界检出：合法预留必须全部"未实现"（本册只登记位，无实现体）。
    pub fn all_unimplemented(&self) -> bool {
        self.slots.iter().all(|s| s.legal_reserve() || s.occupied)
    }
}

impl Default for ReserveBook {
    fn default() -> Self {
        Self::new()
    }
}

// ── 预留纪律（不预写）────────────────────────────────────────────────

/// 纪律条款。
pub const DISCIPLINE: [&str; 3] = ["只留接口位", "只留断言位", "不预写实现"];

/// 预写违例检出：登记表内不得出现实现体指纹。
#[derive(Debug, Clone)]
pub struct PrewriteScan {
    /// 违例位。
    pub violations: Vec<ReservePort>,
}

pub fn scan_prewrite(slots: &[PortSlot], impl_marks: &[ReservePort]) -> PrewriteScan {
    let violations = slots
        .iter()
        .filter(|s| impl_marks.iter().any(|m| *m == s.port && !s.occupied))
        .map(|s| s.port)
        .collect();
    PrewriteScan { violations }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves232_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves232_s09reserve");

    // 01 四口清单守恒
    {
        chk(&mut set, "C3978-01", PORTS.len() == 4 && ReservePort::NewAidTech as usize == 0 && ReservePort::NewInputForm as usize == 3, "err");
    }
    // 02 四口全留位
    {
        let mut b = ReserveBook::new();
        for p in PORTS.iter() {
            b.reserve(*p);
        }
        chk(&mut set, "C3978-02", b.missing().is_empty() && b.slots.len() == 4 && b.all_unimplemented(), "err");
    }
    // 03 留位幂等
    {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewColorTier);
        b.reserve(ReservePort::NewColorTier);
        chk(&mut set, "C3978-03", b.slots.len() == 1, "err");
    }
    // 04 登记缺检出→补登
    {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewFlowMetric);
        chk(&mut set, "C3978-04", b.missing().len() == 3 && { b.reserve(ReservePort::NewAidTech); b.missing().len() == 2 }, "err");
    }
    // 05 预留位被占→迁移评审触发
    {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewAidTech);
        chk(&mut set, "C3978-05", b.occupy(ReservePort::NewAidTech) == S9RCode::SLOT_OCCUPIED && !b.slots[0].legal_reserve(), "err");
    }
    // 06 迁移评审重开新位
    {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewAidTech);
        b.occupy(ReservePort::NewAidTech);
        chk(&mut set, "C3978-06", b.migrate(ReservePort::NewAidTech, ReservePort::NewInputForm) == S9RCode::OK
            && b.slots[0].port == ReservePort::NewInputForm && b.slots[0].legal_reserve(), "err");
    }
    // 07 迁移目标被占拒收
    {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewAidTech);
        b.reserve(ReservePort::NewColorTier);
        b.occupy(ReservePort::NewColorTier);
        chk(&mut set, "C3978-07", b.migrate(ReservePort::NewAidTech, ReservePort::NewColorTier) == S9RCode::SLOT_OCCUPIED, "err");
    }
    // 08 占不存在的位拒收
    {
        let mut b = ReserveBook::new();
        chk(&mut set, "C3978-08", b.occupy(ReservePort::NewFlowMetric) == S9RCode::REG_MISSING, "err");
    }
    // 09 预写违例检出
    {
        let slots = vec![PortSlot { port: ReservePort::NewFlowMetric, iface: true, assertion: true, occupied: false }];
        let scan = scan_prewrite(&slots, &[ReservePort::NewFlowMetric]);
        chk(&mut set, "C3978-09", scan.violations == vec![ReservePort::NewFlowMetric], "err");
    }
    // 10 占用态不判预写
    {
        let slots = vec![PortSlot { port: ReservePort::NewFlowMetric, iface: true, assertion: true, occupied: true }];
        let scan = scan_prewrite(&slots, &[ReservePort::NewFlowMetric]);
        chk(&mut set, "C3978-10", scan.violations.is_empty(), "err");
    }
    // 11 纪律条款守恒
    {
        chk(&mut set, "C3978-11", DISCIPLINE.len() == 3 && DISCIPLINE[2] == "不预写实现", "err");
    }
    // 12 接口位+断言位双齐断言
    {
        let s1 = PortSlot { port: ReservePort::NewAidTech, iface: true, assertion: false, occupied: false };
        let s2 = PortSlot { port: ReservePort::NewAidTech, iface: true, assertion: true, occupied: false };
        chk(&mut set, "C3978-12", !s1.legal_reserve() && s2.legal_reserve(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3978-13", S9RCode::PORT_OVER.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9RCode::OK, S9RCode::PREWRITE, S9RCode::SLOT_OCCUPIED, S9RCode::REG_MISSING, S9RCode::PORT_OVER];
        chk(&mut set, "C3978-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_reserve_all_green() {
        let s = run_ves232_checks();
        assert!(s.all_passed(), "ves232 red: {}", s.red_items_text());
    }

    #[test]
    fn test_occupy_migrate_cycle() {
        let mut b = ReserveBook::new();
        b.reserve(ReservePort::NewInputForm).ok();
        assert!(b.slots[0].legal_reserve());
        b.occupy(ReservePort::NewInputForm).ok();
        assert!(!b.slots[0].legal_reserve());
        b.migrate(ReservePort::NewInputForm, ReservePort::NewAidTech).ok();
        assert!(b.slots[0].legal_reserve());
        assert_eq!(b.slots[0].port, ReservePort::NewAidTech);
    }

    #[test]
    fn test_scan_clean_state() {
        let slots: Vec<PortSlot> = PORTS.iter().map(|p| PortSlot { port: *p, iface: true, assertion: true, occupied: false }).collect();
        let scan = scan_prewrite(&slots, &[]);
        assert!(scan.violations.is_empty());
    }
}
