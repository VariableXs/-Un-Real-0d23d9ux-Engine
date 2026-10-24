//! veb340_gclose —— VE-F0340 组收口移交。
//! 锚点：VE-F0340 · 目标 300 行。
//! 判据原文：实验档收口（自检入树：可验证的登记与骨架，非
//!           生产可用承诺）、风险清单与阶段判定如实入档、
//!           双签移交（双签后移交软渲组）、经验包（生态三
//!           态盘点法 / fake 先行开发 / 实验档边界管理）、
//!           缺项阻断、判据。
//! 组件：收口清单 + 双签闸 + 经验包。
//! 职责定位：组收口是「交接班的签字本」——清单逐项核，缺一
//!           项就阻断；双签是双人复核：一个人签字不算数，
//!           两个名字都在才算交接完成。
//! 性能逐项分解：收口一次性 O(条目)；证据 O(1) 指针。
//! 诊断码段：0x31E00（0x31E01~0x31E09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const GV_VERSION: &str = "veb340_gclose-v1";

/// 诊断段基址（0x31E00 段 · 本模块独占）。
pub const GV_SEG_BASE: u32 = 0x31E00;
pub const XC_MISSING: u32 = 0x31E01; // 收口缺项（阻断）
pub const XC_SAMESIG: u32 = 0x31E02; // 双签同人
pub const XC_REOPEN: u32 = 0x31E03; // 移交后阶段跃迁需重开评估
pub const XC_NOHAND: u32 = 0x31E04; // 未双签即移交
pub const XC_DUP: u32 = 0x31E05; // 清单项重复

/// 收口清单条目（实验档证据标准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// 登记可验证（盘点账在册）。
    Ledger,
    /// 骨架可验证（测试骨架在册）。
    Skeleton,
    /// 风险清单入档。
    Risks,
    /// 阶段判定入档。
    Phase,
    /// 无障碍承诺核验。
    Access,
    /// 非生产可用承诺（诚实声明）。
    NoProd,
}

/// 全部必核项。
pub const ITEMS_ALL: [Item; 6] = [
    Item::Ledger,
    Item::Skeleton,
    Item::Risks,
    Item::Phase,
    Item::Access,
    Item::NoProd,
];

/// 经验包条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lesson {
    /// 生态三态盘点法。
    Triage,
    /// fake 先行开发。
    FakeFirst,
    /// 实验档边界管理。
    ExpBound,
}

/// 收口簿。
pub struct ClosePack {
    /// 已核项位图（bit i = ITEMS_ALL[i]）。
    done: u32,
    /// 双签（a, b）。
    sig: Option<(u32, u32)>,
    /// 移交完成。
    pub handed: bool,
    lessons: Vec<Lesson>,
    /// 入档凭证指针（风险账 id + 阶段判定 id）。
    pub evid: (u32, u32),
}

impl ClosePack {
    /// 新簿。
    pub fn new(risk_id: u32, phase_id: u32) -> ClosePack {
        ClosePack {
            done: 0,
            sig: None,
            handed: false,
            lessons: Vec::new(),
            evid: (risk_id, phase_id),
        }
    }

    /// 核一项：重复核幂等通过（不拒）。
    pub fn check(&mut self, it: Item) -> u32 {
        for (i, x) in ITEMS_ALL.iter().enumerate() {
            if *x == it {
                self.done |= 1 << i;
            }
        }
        self.done
    }

    /// 缺项清单：未核项数。
    pub fn missing(&self) -> u32 {
        let mut n = 0;
        for i in 0..ITEMS_ALL.len() {
            if self.done & (1 << i) == 0 {
                n += 1;
            }
        }
        n
    }

    /// 齐备核。
    pub fn complete(&self) -> bool {
        self.missing() == 0
    }

    /// 双签：同人拒、缺项拒。
    pub fn sign(&mut self, a: u32, b: u32) -> Result<(), u32> {
        if a == b {
            return Err(XC_SAMESIG);
        }
        if !self.complete() {
            return Err(XC_MISSING);
        }
        self.sig = Some((a, b));
        Ok(())
    }

    /// 双签在位核。
    pub fn signed(&self) -> bool {
        self.sig.is_some()
    }

    /// 移交：未双签拒；双签后置移交态。
    pub fn handover(&mut self) -> Result<(), u32> {
        if self.sig.is_none() {
            return Err(XC_NOHAND);
        }
        if self.handed {
            return Err(XC_REOPEN);
        }
        self.handed = true;
        Ok(())
    }

    /// 移交后阶段跃迁 → 重开评估（显性状态机：重开即清签）。
    pub fn reopen_eval(&mut self) -> Result<(), u32> {
        if !self.handed {
            return Err(XC_NOHAND);
        }
        self.handed = false;
        self.sig = None;
        Ok(())
    }

    /// 经验包收录：重复收录拒（一条经验收一次）。
    pub fn learn(&mut self, l: Lesson) -> Result<(), u32> {
        if self.lessons.contains(&l) {
            return Err(XC_DUP);
        }
        self.lessons.push(l);
        Ok(())
    }

    /// 经验数。
    pub fn lessons_len(&self) -> usize {
        self.lessons.len()
    }
}

pub fn run_veb340_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0340");
    // 族零：缺项阻断。
    let mut p = ClosePack::new(21, 7);
    s.add("空簿缺六", p.missing() == 6, "全未核");
    s.add("未齐即签拒", p.sign(1, 2) == Err(XC_MISSING), "缺项阻断");
    s.add("未签移交拒", p.handover() == Err(XC_NOHAND), "无签不交");
    // 族一：逐项核。
    s.add("核登记", p.check(Item::Ledger) != 0, "bit0");
    s.add("缺五回落", p.missing() == 5, "少一缺");
    s.add("重核幂等", p.check(Item::Ledger) == p.check(Item::Ledger), "不翻倍");
    s.add("仍缺五", p.missing() == 5, "幂等不误");
    let _ = p.check(Item::Skeleton);
    let _ = p.check(Item::Risks);
    let _ = p.check(Item::Phase);
    let _ = p.check(Item::Access);
    s.add("仍缺一", p.missing() == 1, "差 NoProd");
    let _ = p.check(Item::NoProd);
    s.add("齐备", p.complete(), "六项全核");
    // 族二：双签。
    s.add("同人拒", p.sign(5, 5) == Err(XC_SAMESIG), "一人两票");
    s.add("双签过", p.sign(5, 6).is_ok(), "两人复核");
    s.add("签在", p.signed(), "在位");
    // 族三：移交。
    s.add("移交过", p.handover().is_ok(), "交接完成");
    s.add("再移交拒", p.handover() == Err(XC_REOPEN), "已交");
    // 族四：移交后阶段跃迁重开。
    s.add("重开过", p.reopen_eval().is_ok(), "跃迁重评");
    s.add("重开清签", !p.signed() && !p.handed, "签清态回");
    s.add("未交重开拒", p.reopen_eval() == Err(XC_NOHAND), "没交开什么");
    // 重签重交（证据指针不变）。
    s.add("凭证在", p.evid == (21, 7), "风险 21 阶段 7");
    let mut p2 = ClosePack::new(21, 7);
    // 全核重走一遍。
    for it in ITEMS_ALL.iter() {
        let _ = p2.check(*it);
    }
    let _ = p2.sign(8, 9);
    s.add("重交过", p2.handover().is_ok(), "二次交接");
    // 族五：经验包。
    s.add("收三态", p2.learn(Lesson::Triage).is_ok(), "盘点法");
    s.add("收 fake", p2.learn(Lesson::FakeFirst).is_ok(), "先行开发");
    s.add("收边界", p2.learn(Lesson::ExpBound).is_ok(), "边界管理");
    s.add("重收拒", p2.learn(Lesson::Triage) == Err(XC_DUP), "一条一次");
    s.add("经验三", p2.lessons_len() == 3, "包满");
    // 族六：断言双签终态。
    s.add("p2 已交", p2.handed, "终态");
    s.add("魔数核", GV_SEG_BASE == 0x31E00, "段基址");
    s
}
