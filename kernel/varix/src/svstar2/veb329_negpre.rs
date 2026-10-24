//! veb329negpre —— VE-F0329 能力协商接口预留。
//! 锚点（CoRun Unxreal 主册）：VE-F0329 · 能力协商接口预留 · 目标 300 行
//! 判据原文：VE-F0329-J1 协商接口账：接口预留（协商入口 /
//!           结果出口两钩子）；能力位表（双方各一 32 位表）；
//!           预留占位（两钩子可空——空钩子调用返回未接 |
//!           记号）；协商核（双方位集与 = 共有能力）；钩子
//!           注册（空 → 已注册）；注册后协商（经钩子转发）；
//!           重注册拒（钩子已占再注册拒）；注销（钩子可退
//!           回空态）。
//! 组件：双钩子 + 位集协商 + 注册表。
//! 职责定位：能力协商接口是「实验线的预留插座」——两钩子
//!           是两个插座：没插东西（空钩子）就报「未接」不
//!           假装通电；与集是插座的对插：两边都有的针脚才
//!           通电；重注册拒是一孔一插：插着一个再插一个是
//!           灾难片开头。
//! 性能逐项分解：协商 O(1)。
//! 诊断码段：0x31B40（0x31B41~0x31B49 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 模块版本。
pub const NP_VERSION: &str = "veb329negpre-v1";

/// 诊断段基址（0x31B40 段 · 本模块独占）。
pub const NP_SEG_BASE: u32 = 0x31B40;
pub const XC_MAGIC: u32 = 0x31B41; // 账魔数破
pub const XC_EMPTY_HOOK: u32 = 0x31B42; // 钩子未接
pub const XC_HOOKED: u32 = 0x31B43; // 钩子已占

/// 账魔数。
const MAGIC: u32 = 0x31B4_1B30;

/// 钩子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    /// 空（未接）。
    Empty,
    /// 已接（持有句柄）。
    Bound(u32),
}

/// 协商结果。
#[derive(Debug, PartialEq, Eq)]
pub struct NegOut {
    /// 共有能力位集。
    pub common: u32,
    /// 双方都零能力？
    pub void: bool,
}

/// 协商接口簿。
pub struct NegBook {
    /// 入口钩子。
    pub in_hook: Hook,
    /// 出口钩子。
    pub out_hook: Hook,
    magic: u32,
}

impl NegBook {
    /// 建：双钩空。
    pub fn new() -> NegBook {
        NegBook { in_hook: Hook::Empty, out_hook: Hook::Empty, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 注册入口钩子。
    pub fn bind_in(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.in_hook != Hook::Empty {
            return Err(XC_HOOKED);
        }
        self.in_hook = Hook::Bound(h);
        Ok(())
    }

    /// 注册出口钩子。
    pub fn bind_out(&mut self, h: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.out_hook != Hook::Empty {
            return Err(XC_HOOKED);
        }
        self.out_hook = Hook::Bound(h);
        Ok(())
    }

    /// 注销。
    pub fn unbind(&mut self, which: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if which == 0 {
            self.in_hook = Hook::Empty;
        } else {
            self.out_hook = Hook::Empty;
        }
        Ok(())
    }

    /// 核心协商：双方位集与。空能力表（0）→ void。
    pub fn negotiate_core(a: u32, b: u32) -> NegOut {
        let common = a & b;
        NegOut { common, void: common == 0 }
    }

    /// 经接口协商：双钩须已接，否则 Err。
    pub fn negotiate(&self, a: u32, b: u32) -> Result<NegOut, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.in_hook == Hook::Empty || self.out_hook == Hook::Empty {
            return Err(XC_EMPTY_HOOK);
        }
        Ok(Self::negotiate_core(a, b))
    }
}

pub fn run_veb329_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0329");
    let mut b = NegBook::new();
    // 族零：空钩调用。
    s.add("空钩协商拒", b.negotiate(0xFF, 0x0F) == Err(XC_EMPTY_HOOK), "未接");
    // 族一：核心协商。
    let o1 = NegBook::negotiate_core(0xFF00, 0x0FF0);
    s.add("与集 0x0F00", o1.common == 0x0F00, "共有");
    s.add("非空", o1.void == false, "有通");
    let o2 = NegBook::negotiate_core(0xF0, 0x0F);
    s.add("零与 void", o2.void && o2.common == 0, "全无通");
    s.add("自协商", NegBook::negotiate_core(0xAB, 0xAB).common == 0xAB, "自与");
    // 族二：注册。
    s.add("接入入口", b.bind_in(100).is_ok(), "一插");
    s.add("重入拒", b.bind_in(200) == Err(XC_HOOKED), "一孔一插");
    s.add("出仍空拒", b.negotiate(0xFF, 0x0F) == Err(XC_EMPTY_HOOK), "另一孔空");
    s.add("接出口", b.bind_out(300).is_ok(), "二插");
    // 族三：注册后协商。
    s.add("过钩协商", b.negotiate(0xFF00, 0x0FF0) == Ok(o1), "转发");
    s.add("过钩 void", b.negotiate(0xF0, 0x0F) == Ok(o2), "零通也过");
    // 钩子句柄在位。
    s.add("句柄 100", b.in_hook == Hook::Bound(100), "入口");
    s.add("句柄 300", b.out_hook == Hook::Bound(300), "出口");
    // 族四：注销与重接。
    s.add("退出口", b.unbind(1).is_ok(), "拔出");
    s.add("退后空", b.out_hook == Hook::Empty, "空态");
    s.add("退后协商拒", b.negotiate(0xFF, 0x0F) == Err(XC_EMPTY_HOOK), "缺一孔");
    s.add("重接新柄", b.bind_out(400).is_ok(), "新插座");
    s.add("新柄在位", b.out_hook == Hook::Bound(400), "句柄换");
    // 注销幂等（空注空）。
    s.add("空注幂等", b.unbind(0).is_ok() && b.unbind(0).is_ok(), "再拔");
    s.add("入口已空", b.in_hook == Hook::Empty, "双空");
    // 族五：协商位与性质。
    s.add("与交换律", NegBook::negotiate_core(0xA5, 0x5A).common == NegBook::negotiate_core(0x5A, 0xA5).common, "交换");
    s.add("子集过", NegBook::negotiate_core(0xFF, 0x0F).common == 0x0F, "子集为共");
    s.add("魔数核", NP_SEG_BASE == 0x31B40, "段基址");
    s
}
