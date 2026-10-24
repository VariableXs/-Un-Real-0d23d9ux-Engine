//! VE-F3998 · S 域演进预留（域级）
//!
//! 预留：S 域级演进预留（F3978 组级四口之上域级增三口：新辅助技术协
//! 议口 / 新感知模态口（触觉/听觉扩展）/ 新认知辅助口）+ 预留纪律复述
//! （不预写）。
//! 数据结构：域级预留册（三口）；接口位；断言位。
//! 错误路径：预写越界→回退；口被占→迁移评审；登记缺→补登。
//! 性能：登记 O(1)；评审 O(口)；断言 O(位)。
//! 跨批对接：F3978 组级上游；T 域衔接；家族预留同构。
//! 无障碍与隐私：三口均无障碍向（域本色）；预留不引入新隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x4AC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x4AC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T18RCode(pub u16);

impl T18RCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const PREWRITE_OVER: Self = Self(SEG_BASE + 1);
    pub const SLOT_OCCUPIED: Self = Self(SEG_BASE + 2);
    pub const REG_MISS: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::PREWRITE_OVER => "预写越界→回退",
            Self::SLOT_OCCUPIED => "口被占→迁移评审",
            Self::REG_MISS => "登记缺→补登",
            _ => "未知",
        }
    }
}

// ── 域级三口 ─────────────────────────────────────────────────────────

/// 域级预留三口（均无障碍向——域本色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainSlot {
    /// 新辅助技术协议口。
    AtProtocol,
    /// 新感知模态口（触觉/听觉扩展）。
    SenseModal,
    /// 新认知辅助口。
    CognitiveAid,
}

pub const DOMAIN_SLOTS: [DomainSlot; 3] = [
    DomainSlot::AtProtocol, DomainSlot::SenseModal, DomainSlot::CognitiveAid,
];

impl DomainSlot {
    pub fn name(self) -> &'static str {
        match self {
            DomainSlot::AtProtocol => "新辅助技术协议口",
            DomainSlot::SenseModal => "新感知模态口",
            DomainSlot::CognitiveAid => "新认知辅助口",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            DomainSlot::AtProtocol => 0,
            DomainSlot::SenseModal => 1,
            DomainSlot::CognitiveAid => 2,
        }
    }
}

/// 预留项：接口位（可挂载）+ 断言位（可运行）+ 占用状态。
#[derive(Debug, Clone)]
pub struct ReserveItem {
    /// 接口位描述（空 = 预写——纪律禁止）。
    pub iface_desc: String,
    /// 断言位描述。
    pub assert_desc: String,
    /// 占用者（空 = 未占）。
    pub occupant: String,
    /// 迁移评审过。
    pub migrated: bool,
}

/// 域级预留册。
#[derive(Debug, Clone)]
pub struct DomainReserveBook {
    pub items: [Option<ReserveItem>; 3],
}

/// 预留纪律：接口位/断言位描述须为占位声明（不预写实现细节——检测
/// 关键字越界）。
const PREWRITE_FORBIDDEN: [&str; 3] = ["实现", "算法细节", "协议字段"];

impl DomainReserveBook {
    pub fn new() -> Self {
        Self { items: [None, None, None] }
    }

    /// 补登一口（占位声明式）。
    pub fn register(&mut self, slot: DomainSlot, iface_desc: &str, assert_desc: &str) -> T18RCode {
        if self.items[slot.slot()].is_some() {
            return T18RCode::REG_MISS;
        }
        // 预写越界检测。
        for w in PREWRITE_FORBIDDEN.iter() {
            if iface_desc.contains(w) || assert_desc.contains(w) {
                return T18RCode::PREWRITE_OVER;
            }
        }
        self.items[slot.slot()] = Some(ReserveItem {
            iface_desc: String::from(iface_desc),
            assert_desc: String::from(assert_desc),
            occupant: String::new(),
            migrated: false,
        });
        T18RCode::OK
    }

    /// 登记缺检出。
    pub fn missing(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for s in DOMAIN_SLOTS.iter() {
            if self.items[s.slot()].is_none() {
                out.push(s.name());
            }
        }
        out
    }

    /// 占用（口被占 → 迁移评审）。
    pub fn occupy(&mut self, slot: DomainSlot, occupant: &str) -> T18RCode {
        match self.items[slot.slot()].as_mut() {
            Some(item) => {
                if !item.occupant.is_empty() {
                    return T18RCode::SLOT_OCCUPIED;
                }
                item.occupant = String::from(occupant);
                T18RCode::OK
            }
            None => T18RCode::REG_MISS,
        }
    }

    /// 迁移评审（占用者替换前必须过）。
    pub fn migrate(&mut self, slot: DomainSlot, new_occ: &str) -> T18RCode {
        match self.items[slot.slot()].as_mut() {
            Some(item) => {
                if item.occupant.is_empty() {
                    return T18RCode::REG_MISS;
                }
                item.occupant = String::from(new_occ);
                item.migrated = true;
                T18RCode::OK
            }
            None => T18RCode::REG_MISS,
        }
    }

    /// 三口齐备（登记 + 断言位非空）。
    pub fn complete(&self) -> bool {
        self.missing().is_empty()
            && self.items.iter().flatten().all(|i| !i.assert_desc.is_empty())
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves252_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves252_s10reserve");

    // C3998-01 诊断码段不越窗。
    chk(&mut set, "C3998-01",
        T18RCode::REG_MISS.0 < SEG_BASE + 16 && SEG_BASE == 0x4AC0, "err");

    // C3998-02 三口齐备（域本色——均无障碍向）。
    chk(&mut set, "C3998-02",
        DOMAIN_SLOTS.len() == 3
            && DOMAIN_SLOTS[0].name() == "新辅助技术协议口"
            && DOMAIN_SLOTS[2].name() == "新认知辅助口", "err");

    // C3998-03 登记缺检出。
    let mut b = DomainReserveBook::new();
    chk(&mut set, "C3998-03", b.missing().len() == 3 && !b.complete(), "err");

    // C3998-04 占位声明式登记过。
    chk(&mut set, "C3998-04",
        b.register(DomainSlot::AtProtocol, "占位：AT 协议接口位", "占位：协议握手断言位")
            == T18RCode::OK, "err");

    // C3998-05 预写越界（描述含实现细节）→ 回退。
    chk(&mut set, "C3998-05",
        b.register(DomainSlot::SenseModal, "占位：触觉实现细节如下", "x")
            == T18RCode::PREWRITE_OVER, "err");

    // C3998-06 修正描述后登记过。
    chk(&mut set, "C3998-06",
        b.register(DomainSlot::SenseModal, "占位：触觉接口位", "占位：触觉断言位")
            == T18RCode::OK, "err");

    // C3998-07 三口齐 → complete。
    b.register(DomainSlot::CognitiveAid, "占位：认知辅助接口位", "占位：认知断言位");
    chk(&mut set, "C3998-07", b.complete() && b.missing().is_empty(), "err");

    // C3998-08 重复登记拒绝。
    chk(&mut set, "C3998-08",
        b.register(DomainSlot::AtProtocol, "再登", "再登") == T18RCode::REG_MISS, "err");

    // C3998-09 占用 → OK。
    chk(&mut set, "C3998-09",
        b.occupy(DomainSlot::AtProtocol, "盲文协议 v2") == T18RCode::OK, "err");

    // C3998-10 口被占 → 迁移评审信号。
    chk(&mut set, "C3998-10",
        b.occupy(DomainSlot::AtProtocol, "新占用者") == T18RCode::SLOT_OCCUPIED, "err");

    // C3998-11 迁移评审后换占用者。
    chk(&mut set, "C3998-11",
        b.migrate(DomainSlot::AtProtocol, "盲文协议 v3") == T18RCode::OK
            && b.items[0].as_ref().map(|i| i.migrated).unwrap_or(false), "err");

    // C3998-12 未占口迁移 → REG_MISS。
    chk(&mut set, "C3998-12",
        b.migrate(DomainSlot::CognitiveAid, "x") == T18RCode::REG_MISS, "err");

    // C3998-13 未登口占用 → REG_MISS。
    let mut b2 = DomainReserveBook::new();
    chk(&mut set, "C3998-13",
        b2.occupy(DomainSlot::SenseModal, "x") == T18RCode::REG_MISS, "err");

    // C3998-14 预写禁词表三词齐备。
    chk(&mut set, "C3998-14",
        PREWRITE_FORBIDDEN.contains(&"实现")
            && PREWRITE_FORBIDDEN.contains(&"算法细节")
            && PREWRITE_FORBIDDEN.contains(&"协议字段"), "err");

    // C3998-15 断言位空描述登记仍算齐（断言位空在 complete 检）。
    let mut b3 = DomainReserveBook::new();
    for s in DOMAIN_SLOTS.iter() {
        b3.register(*s, "占位接口", "");
    }
    chk(&mut set, "C3998-15",
        b3.missing().is_empty() && !b3.complete(), "err");

    // C3998-16 补断言位后齐。
    let mut b4 = DomainReserveBook::new();
    for s in DOMAIN_SLOTS.iter() {
        b4.register(*s, "占位接口", "占位断言");
    }
    chk(&mut set, "C3998-16", b4.complete(), "err");

    // C3998-17 槽位双射。
    let slots: Vec<usize> = DOMAIN_SLOTS.iter().map(|s| s.slot()).collect();
    chk(&mut set, "C3998-17",
        slots[0] != slots[1] && slots[1] != slots[2] && slots[0] != slots[2], "err");

    // C3998-18 占用态与迁移态解耦。
    let mut b5 = DomainReserveBook::new();
    b5.register(DomainSlot::CognitiveAid, "i", "a");
    b5.occupy(DomainSlot::CognitiveAid, "first");
    b5.migrate(DomainSlot::CognitiveAid, "second");
    chk(&mut set, "C3998-18",
        b5.items[2].as_ref().map(|i| i.occupant == "second" && i.migrated).unwrap_or(false), "err");

    // C3998-19 预写越界码语义。
    chk(&mut set, "C3998-19", T18RCode::PREWRITE_OVER.explain().contains("回退"), "err");

    // C3998-20 错误码互异。
    let codes = [T18RCode::OK.0, T18RCode::PREWRITE_OVER.0, T18RCode::SLOT_OCCUPIED.0,
        T18RCode::REG_MISS.0];
    chk(&mut set, "C3998-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prewrite_blocked() {
        let mut b = DomainReserveBook::new();
        assert_eq!(
            b.register(DomainSlot::AtProtocol, "含实现细节", "a"),
            T18RCode::PREWRITE_OVER
        );
    }

    #[test]
    fn occupy_and_migrate() {
        let mut b = DomainReserveBook::new();
        b.register(DomainSlot::SenseModal, "i", "a");
        assert_eq!(b.occupy(DomainSlot::SenseModal, "a1"), T18RCode::OK);
        assert_eq!(b.occupy(DomainSlot::SenseModal, "a2"), T18RCode::SLOT_OCCUPIED);
        assert_eq!(b.migrate(DomainSlot::SenseModal, "a2"), T18RCode::OK);
    }
}
