//! veb355_allgreen —— VE-F0355 软渲一致性套件全绿。
//! 锚点：VE-F0355 · 目标 380 行。
//! 判据原文：四件全绿（总对拍语义清零 / 单元套件全过 / 伸缩
//!           结论入档 / 4K 判据达标）、豁免显性（无理由豁免
//!           视为未绿）、证据裁决（核验结论与证据矛盾以证据
//!           为准）、收官阻断（任一件未绿阻断）、判据。
//! 组件：核验清单 + 豁免账 + 收官闸。
//! 职责定位：全绿核验是「毕业答辩四门课」——对拍、单元、伸
//!           缩、4K 一门不过都不发证；想走后门（豁免）就得
//!           把理由写进会议纪要，没理由的豁免等于没过。
//! 性能逐项分解：核验 O(件数)；证据 O(1) 指针；豁免 O(1)。
//! 诊断码段：0x321C0（0x321C1~0x321C9 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AG_VERSION: &str = "veb355_allgreen-v1";

/// 诊断段基址（0x321C0 段 · 本模块独占）。
pub const AG_SEG_BASE: u32 = 0x321C0;
pub const XC_NOTGREEN: u32 = 0x321C1; // 未绿（收官阻断）
pub const XC_BAREX: u32 = 0x321C2; // 无理由豁免
pub const XC_DUP: u32 = 0x321C3; // 重复核验
pub const XC_NOEV: u32 = 0x321C4; // 无证据指针

/// 四件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// 总对拍（语义级清零或显性豁免）。
    Ab,
    /// 单元套件（软渲档全过）。
    Unit,
    /// 伸缩验证（结论入档）。
    Scale,
    /// 4K 判据（实测达标）。
    Res4K,
}

/// 件序。
pub fn item_no(i: Item) -> u32 {
    match i {
        Item::Ab => 0,
        Item::Unit => 1,
        Item::Scale => 2,
        Item::Res4K => 3,
    }
}

/// 核验条目。
pub struct Entry {
    pub it: Item,
    /// 证据指针（非零 = 有效）。
    pub evid: u32,
    /// 豁免理由（None = 未豁免）。
    pub exempt: Option<u32>,
}

/// 核验清单。
pub struct GreenBook {
    entries: Vec<Entry>,
    /// 收官宣告。
    pub declared: bool,
}

impl GreenBook {
    /// 空簿。
    pub fn new() -> GreenBook {
        GreenBook {
            entries: Vec::new(),
            declared: false,
        }
    }

    /// 过一件：证据指针必填。
    pub fn pass(&mut self, it: Item, evid: u32) -> Result<(), u32> {
        if evid == 0 {
            return Err(XC_NOEV);
        }
        if self.entries.iter().any(|e| e.it == it && e.exempt.is_none()) {
            return Err(XC_DUP);
        }
        // 豁免态转正：以证据裁决覆盖豁免。
        self.entries.retain(|e| e.it != it);
        self.entries.push(Entry {
            it,
            evid,
            exempt: None,
        });
        Ok(())
    }

    /// 豁免：必须带理由（理由代号非零）。
    pub fn exempt(&mut self, it: Item, reason: u32) -> Result<(), u32> {
        if reason == 0 {
            return Err(XC_BAREX);
        }
        self.entries.retain(|e| e.it != it);
        self.entries.push(Entry {
            it,
            evid: 0,
            exempt: Some(reason),
        });
        Ok(())
    }

    /// 单件绿核：过（有证据）或显性豁免（有理由）。
    pub fn green_of(&self, it: Item) -> bool {
        self.entries
            .iter()
            .any(|e| e.it == it && (e.evid != 0 || e.exempt.is_some()))
    }

    /// 四件全绿。
    pub fn all_green(&self) -> bool {
        [Item::Ab, Item::Unit, Item::Scale, Item::Res4K]
            .iter()
            .all(|i| self.green_of(*i))
    }

    /// 未绿件数。
    pub fn red_count(&self) -> u32 {
        4 - [Item::Ab, Item::Unit, Item::Scale, Item::Res4K]
            .iter()
            .filter(|i| self.green_of(**i))
            .count() as u32
    }

    /// 收官宣告：全绿才许；重复宣告拒。
    pub fn declare(&mut self) -> Result<(), u32> {
        if self.declared {
            return Err(XC_DUP);
        }
        if !self.all_green() {
            return Err(XC_NOTGREEN);
        }
        self.declared = true;
        Ok(())
    }

    /// 豁免件清单。
    pub fn exempted(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|e| e.exempt.is_some())
            .map(|e| item_no(e.it))
            .collect()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

pub fn run_veb355_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0355");
    // 族零：证据门。
    let mut b = GreenBook::new();
    s.add("零证据拒", b.pass(Item::Ab, 0) == Err(XC_NOEV), "无指针");
    s.add("过对拍", b.pass(Item::Ab, 101).is_ok(), "语义清零");
    s.add("重复拒", b.pass(Item::Ab, 102) == Err(XC_DUP), "一件一核");
    // 族一：逐件。
    s.add("过单元", b.pass(Item::Unit, 202).is_ok(), "软渲档全过");
    s.add("过伸缩", b.pass(Item::Scale, 303).is_ok(), "结论入档");
    s.add("未满三件", b.red_count() == 1, "差 4K");
    s.add("未全绿", !b.all_green(), "收官未到");
    s.add("缺件宣告拒", b.declare() == Err(XC_NOTGREEN), "阻断");
    // 族二：豁免。
    s.add("无理由豁免拒", b.exempt(Item::Res4K, 0) == Err(XC_BAREX), "裸豁免");
    s.add("豁免过", b.exempt(Item::Res4K, 9).is_ok(), "理由 9");
    s.add("豁免即绿", b.all_green(), "显性豁免算绿");
    s.add("豁免单", b.exempted() == vec![3u32], "4K 豁免");
    s.add("条目四", b.len() == 4, "四件齐");
    // 族三：证据裁决覆盖豁免。
    s.add("补测转正", b.pass(Item::Res4K, 404).is_ok(), "证据推翻豁免");
    s.add("豁免清空", b.exempted().is_empty(), "转正后无豁免");
    s.add("证据在", b.green_of(Item::Res4K), "4K 有据");
    // 族四：收官。
    s.add("宣告过", b.declare().is_ok(), "全绿收官");
    s.add("宣告在", b.declared, "入证据");
    s.add("重复宣告拒", b.declare() == Err(XC_DUP), "一次");
    // 族五：豁免件不算证据件。
    let mut b2 = GreenBook::new();
    let _ = b2.exempt(Item::Ab, 5);
    let _ = b2.exempt(Item::Unit, 6);
    let _ = b2.exempt(Item::Scale, 7);
    let _ = b2.exempt(Item::Res4K, 8);
    s.add("全豁免也全绿", b2.all_green(), "显性豁免路径");
    s.add("全豁免单", b2.exempted().len() == 4, "四条理由");
    s.add("零证据条目", b2.entries.iter().all(|e| e.evid == 0), "豁免无证据");
    // 族六：空簿。
    s.add("空簿三红", GreenBook::new().red_count() == 4, "全未核");
    s.add("魔数核", AG_SEG_BASE == 0x321C0, "段基址");
    s
}
