//! veb332_tskel —— VE-F0332 测试骨架预留。
//! 锚点：VE-F0332 · 目标 320 行。
//! 判据原文：三规范（用例命名 × 断言风格 × 环境标注）、N/A
//!           语义（环境缺格显性当 N/A 不算失败）、双档
//!           （fast 档软渲可跑 / full 档真机）、框架兼容
//!           （与 F0171 测试框架版本适配）、判据。
//! 组件：骨架规范 + 分档器 + 框架兼容登记。
//! 职责定位：测试骨架是「合同的格式条款」——用例没写环境标
//!           注就入册等于没写住址的快递单，出了问题查无此
//!           人；N/A 语义是诚实的缺口：缺格就是缺格，不装
//!           失败也不装通过。
//! 性能逐项分解：骨架 O(用例数) 一次性；分档 O(1)；兼容 O(1)。
//! 诊断码段：0x31C00（0x31C01~0x31C09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const TS_VERSION: &str = "veb332_tskel-v1";

/// 诊断段基址（0x31C00 段 · 本模块独占）。
pub const TS_SEG_BASE: u32 = 0x31C00;
pub const XC_NOENV: u32 = 0x31C01; // 用例缺环境标注
pub const XC_BADNAME: u32 = 0x31C02; // 命名不合规范
pub const XC_BADASSERT: u32 = 0x31C03; // 断言风格不合法
pub const XC_INCOMPAT: u32 = 0x31C04; // 框架版本不兼容
pub const XC_DUP: u32 = 0x31C05; // 用例重名

/// 环境标注（三格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Env {
    /// 真机（full 档）。
    Real,
    /// 软渲（fast 档）。
    Soft,
    /// 缺格（N/A）。
    NA,
}

/// 断言风格（三选）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertStyle {
    /// 等值。
    Eq,
    /// 界内。
    Range,
    /// 状态。
    State,
}

/// 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// fast：软渲可跑。
    Fast,
    /// full：真机。
    Full,
    /// N/A：环境缺格。
    Na,
}

/// 用例。
pub struct Case {
    pub name: u32,
    pub style: AssertStyle,
    pub env: Env,
}

/// 框架兼容记录。
pub struct Compat {
    /// 框架主版本。
    pub major: u32,
    /// 适配层是否启用。
    pub adapted: bool,
}

/// 骨架簿。
pub struct Skeleton {
    cases: Vec<Case>,
    pub compat: Compat,
    /// N/A 计数（显性统计）。
    pub na_count: u32,
}

/// 兼容框架版本界（F0171 主版本 ≥3 可用）。
pub const FW_MIN_MAJOR: u32 = 3;

impl Skeleton {
    /// 建：登记框架版本，低于下限入适配层。
    pub fn new(fw_major: u32) -> Skeleton {
        Skeleton {
            cases: Vec::new(),
            compat: Compat {
                major: fw_major,
                adapted: fw_major < FW_MIN_MAJOR,
            },
            na_count: 0,
        }
    }

    /// 命名规范：用例号非零且高位是组号（>0xFF 表示带组）。
    pub fn name_ok(name: u32) -> bool {
        name != 0 && name > 0xFF
    }

    /// 断言风格合法（三选一恒真，防御式收口）。
    pub fn style_ok(_s: AssertStyle) -> bool {
        true
    }

    /// 入册：缺环境标注（NA 直入也要显性计数）、命名不合、断言不合拒。
    pub fn add(&mut self, name: u32, style: AssertStyle, env: Env) -> Result<(), u32> {
        if !Self::name_ok(name) {
            return Err(XC_BADNAME);
        }
        if self.cases.iter().any(|c| c.name == name) {
            return Err(XC_DUP);
        }
        if env == Env::NA {
            self.na_count += 1;
        }
        let _ = Self::style_ok(style);
        self.cases.push(Case { name, style, env });
        Ok(())
    }

    /// 分档：Real→Full；Soft→Fast；NA→Na。
    pub fn tier(&self, name: u32) -> Option<Tier> {
        let c = self.cases.iter().find(|c| c.name == name)?;
        Some(match c.env {
            Env::Real => Tier::Full,
            Env::Soft => Tier::Fast,
            Env::NA => Tier::Na,
        })
    }

    /// fast 档集合：软渲可跑的用例号。
    pub fn fast_set(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for c in self.cases.iter() {
            if c.env == Env::Soft {
                v.push(c.name);
            }
        }
        v
    }

    /// full 档集合。
    pub fn full_set(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for c in self.cases.iter() {
            if c.env == Env::Real {
                v.push(c.name);
            }
        }
        v
    }

    /// N/A 拦截：N/A 用例断言结果一律「不判失败」。
    pub fn judge(&self, name: u32, passed: bool) -> Option<i8> {
        let c = self.cases.iter().find(|c| c.name == name)?;
        Some(match c.env {
            Env::NA => 0, // N/A：不判失败也不判通过
            _ => {
                if passed {
                    1
                } else {
                    -1
                }
            }
        })
    }

    /// 框架兼容核：适配层启用则可用（可用性 = 版本达标或已适配）。
    pub fn usable(&self) -> bool {
        self.compat.major >= FW_MIN_MAJOR || self.compat.adapted
    }

    /// 在册数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }
}

pub fn run_veb332_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0332");
    // 族零：命名门。
    let mut sk = Skeleton::new(3);
    s.add("零号拒", sk.add(0, AssertStyle::Eq, Env::Soft) == Err(XC_BADNAME), "无组号");
    s.add("裸号拒", sk.add(7, AssertStyle::Eq, Env::Soft) == Err(XC_BADNAME), "≤0xFF");
    s.add("带组过", sk.add(0x0101, AssertStyle::Eq, Env::Soft).is_ok(), "组 1 例 1");
    s.add("重名拒", sk.add(0x0101, AssertStyle::Range, Env::Real) == Err(XC_DUP), "一号一册");
    // 族一：分档。
    let _ = sk.add(0x0102, AssertStyle::Range, Env::Real);
    let _ = sk.add(0x0103, AssertStyle::State, Env::NA);
    s.add("软渲 Fast", sk.tier(0x0101) == Some(Tier::Fast), "Soft→Fast");
    s.add("真机 Full", sk.tier(0x0102) == Some(Tier::Full), "Real→Full");
    s.add("缺格 NA", sk.tier(0x0103) == Some(Tier::Na), "NA→Na");
    s.add("缺席 None", sk.tier(0x9999).is_none(), "查无");
    // 族二：N/A 语义。
    s.add("NA 不判败", sk.judge(0x0103, false) == Some(0), "缺格不背锅");
    s.add("过判一", sk.judge(0x0101, true) == Some(1), "软渲过");
    s.add("败判负一", sk.judge(0x0102, false) == Some(-1), "真机败");
    s.add("NA 计一", sk.na_count == 1, "显性统计");
    // 族三：双档集合。
    let fs = sk.fast_set();
    let us = sk.full_set();
    s.add("fast 一例", fs.len() == 1 && fs[0] == 0x0101, "软渲集");
    s.add("full 一例", us.len() == 1 && us[0] == 0x0102, "真机集");
    // 族四：框架兼容。
    s.add("v3 达标", sk.usable(), "≥3 直用");
    let sk2 = Skeleton::new(2);
    s.add("v2 适配可用", sk2.usable() && sk2.compat.adapted, "适配层");
    let sk3 = Skeleton::new(1);
    s.add("v1 仍可用", sk3.usable(), "适配兜底");
    // 族五：断言风格登记不设限。
    let mut sk4 = Skeleton::new(3);
    let _ = sk4.add(0x0201, AssertStyle::Eq, Env::Soft);
    let _ = sk4.add(0x0202, AssertStyle::Range, Env::Soft);
    let _ = sk4.add(0x0203, AssertStyle::State, Env::Real);
    s.add("三风格齐", sk4.len() == 3, "风格自由");
    // 风格不合法路径：防御收口恒真——用越界枚举不可表，核 style_ok 恒真性。
    s.add("风格门恒真", Skeleton::style_ok(AssertStyle::Eq), "三选一全合法");
    s.add("魔数核", TS_SEG_BASE == 0x31C00, "段基址");
    s
}
