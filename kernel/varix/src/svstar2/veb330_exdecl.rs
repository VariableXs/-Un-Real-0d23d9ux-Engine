//! veb330_exdecl —— VE-F0330 实验档声明。
//! 锚点（CoRun Unxreal 主册）：VE-F0330 · 实验档声明 · 目标 300 行
//! 判据原文：VE-F0330-J1 实验档账：档位三阶（实验 / 预览 /
//!           稳定）；声明簿（特性名 + 档位 + 实验到期周期）；
//!           实验档纪律（实验特性必须带到期周期 1~12，无周
//!           期拒）；档位晋升（实验到期前评审 → 预览 → 稳
//!           定；晋升只进不退）；到期清退（到期未晋升 → 实
//!           验特性自动撤下 + 立案）；稳定档不可降核（稳定
//!           特性拒绝降档）；重复声明拒。
//! 组件：档位阶 + 到期账 + 晋升闸 + 清退器。
//! 职责定位：实验档声明是「特性的试用期合同」——实验是试用
//!           期：试用期必须写截止日，没截止日的试用是白嫖；
//!           晋升只进不退是职级纪律：转正了不能降回实习生；
//!           到期清退是合同的到期条款：不转正就到期走人。
//! 性能逐项分解：声明 O(1)；清退 O(簿长)。
//! 诊断码段：0x31B80（0x31B81~0x31B89 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EX_VERSION: &str = "veb330_exdecl-v1";

/// 诊断段基址（0x31B80 段 · 本模块独占）。
pub const EX_SEG_BASE: u32 = 0x31B80;
pub const XC_MAGIC: u32 = 0x31B81; // 账魔数破
pub const XC_DUP: u32 = 0x31B82; // 重复声明
pub const XC_TTL: u32 = 0x31B83; // 到期周期不合法
pub const XC_DEMOTE: u32 = 0x31B84; // 稳定档禁降
pub const XC_NOT_EXP: u32 = 0x31B85; // 非实验档操作

/// 账魔数。
const MAGIC: u32 = 0x31B8_1B31;

/// 档位三阶。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 实验。
    Exp,
    /// 预览。
    Preview,
    /// 稳定。
    Stable,
}

/// 实验到期周期界。
pub const TTL_MIN: u32 = 1;
pub const TTL_MAX: u32 = 12;

/// 特性声明。
pub struct Feat {
    pub name: u32,
    pub tier: Tier,
    /// 实验到期周期（仅实验档有效）。
    pub ttl: u32,
    /// 登记周期。
    pub born: u32,
}

/// 声明簿。
pub struct DeclBook {
    pub feats: Vec<Feat>,
    pub evicted: u32,
    magic: u32,
}

impl DeclBook {
    /// 建。
    pub fn new() -> DeclBook {
        DeclBook { feats: Vec::new(), evicted: 0, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn find_mut(&mut self, name: u32) -> Option<&mut Feat> {
        for f in self.feats.iter_mut() {
            if f.name == name {
                return Some(f);
            }
        }
        None
    }

    /// 声明：实验档必须带 1~12 周期；预览/稳定 ttl 忽略。
    pub fn declare(&mut self, name: u32, tier: Tier, ttl: u32, born: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.find_mut(name).is_some() {
            return Err(XC_DUP);
        }
        if tier == Tier::Exp && (ttl < TTL_MIN || ttl > TTL_MAX) {
            return Err(XC_TTL);
        }
        self.feats.push(Feat { name, tier, ttl, born });
        Ok(())
    }

    /// 晋升：实验→预览→稳定，只进不退。
    pub fn promote(&mut self, name: u32) -> Result<Tier, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        let f = match self.find_mut(name) {
            Some(x) => x,
            None => return Err(XC_DUP),
        };
        match f.tier {
            Tier::Exp => {
                f.tier = Tier::Preview;
                Ok(Tier::Preview)
            }
            Tier::Preview => {
                f.tier = Tier::Stable;
                Ok(Tier::Stable)
            }
            Tier::Stable => Err(XC_NOT_EXP),
        }
    }

    /// 降档拒（稳定禁降）。
    pub fn demote(&mut self, _name: u32) -> Result<(), u32> {
        Err(XC_DEMOTE)
    }

    /// 到期清退：实验档 born + ttl ≤ 周期 → 撤下立案。
    pub fn sweep(&mut self, now: u32) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.feats.len() {
            let remove = match &self.feats[i] {
                Feat { tier: Tier::Exp, ttl, born, .. } => born + ttl <= now,
                _ => false,
            };
            if remove {
                self.feats.remove(i);
                self.evicted += 1;
                n += 1;
            } else {
                i += 1;
            }
        }
        n
    }

    /// 查档。
    pub fn tier_of(&self, name: u32) -> Option<Tier> {
        for f in self.feats.iter() {
            if f.name == name {
                return Some(f.tier);
            }
        }
        None
    }

    /// 在簿数。
    pub fn len(&self) -> usize {
        self.feats.len()
    }
}

pub fn run_veb330_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0330");
    let mut b = DeclBook::new();
    // 族零：声明门。
    s.add("实验无期拒", b.declare(1, Tier::Exp, 0, 1) == Err(XC_TTL), "白嫖拒");
    s.add("实验超期拒", b.declare(1, Tier::Exp, 13, 1) == Err(XC_TTL), ">12");
    s.add("实验 1 期过", b.declare(1, Tier::Exp, 1, 1).is_ok(), "最短试用");
    s.add("重声明拒", b.declare(1, Tier::Preview, 0, 1) == Err(XC_DUP), "一号一档");
    s.add("预览无期过", b.declare(2, Tier::Preview, 0, 1).is_ok(), "非实验免期");
    s.add("稳定直过", b.declare(3, Tier::Stable, 0, 1).is_ok(), "天生稳定");
    // 族一：晋升只进。
    s.add("实→预", b.promote(1) == Ok(Tier::Preview), "转正一级");
    s.add("预→稳", b.promote(1) == Ok(Tier::Stable), "再升");
    s.add("稳升拒", b.promote(1) == Err(XC_NOT_EXP), "到顶");
    s.add("档在稳", b.tier_of(1) == Some(Tier::Stable), "在位");
    // 族二：稳定禁降。
    s.add("降档拒", b.demote(1) == Err(XC_DEMOTE), "不降级");
    s.add("档仍稳", b.tier_of(1) == Some(Tier::Stable), "没动");
    // 族三：到期清退。
    // 造三个实验特性。
    let _ = b.declare(10, Tier::Exp, 2, 1); // 到期 3
    let _ = b.declare(11, Tier::Exp, 3, 1); // 到期 4
    let _ = b.declare(12, Tier::Exp, 2, 5); // 到期 7
    // 周期 2：无到期。
    s.add("周 2 零清", b.sweep(2) == 0 && b.len() == 6, "全活");
    // 周期 3：特性 10 到期。
    s.add("周 3 清一", b.sweep(3) == 1 && b.tier_of(10).is_none(), "撤 10");
    // 周期 4：特性 11 到期。
    s.add("周 4 清一", b.sweep(4) == 1 && b.tier_of(11).is_none(), "撤 11");
    // 周期 5：无到期（12 到期 7）。
    s.add("周 5 零清", b.sweep(5) == 0, "全活");
    // 周期 7：特性 12 到期。
    s.add("周 7 清一", b.sweep(7) == 1 && b.tier_of(12).is_none(), "撤 12");
    s.add("清退账 3", b.evicted == 3, "三案");
    // 稳定档永不清。
    s.add("稳定存活", b.tier_of(3) == Some(Tier::Stable), "到期无关");
    // 族四：清退后可重声明。
    s.add("重声明过", b.declare(10, Tier::Exp, 2, 8).is_ok(), "再试用");
    s.add("重清", b.sweep(10) == 1 && b.evicted == 4, "再撤");
    // 族五：簿长核。
    s.add("余三", b.len() == 3, "1,2,3 号");
    s.add("魔数核", EX_SEG_BASE == 0x31B80, "段基址");
    s
}
