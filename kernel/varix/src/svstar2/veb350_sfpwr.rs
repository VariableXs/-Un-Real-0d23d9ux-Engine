//! veb350_sfpwr —— VE-F0350 软渲功耗诚实声明。
//! 锚点：VE-F0350 · 目标 300 行。
//! 判据原文：量级如实（CPU 满载功耗显著高于硬渲，倍数量级
//!           如实呈现不粉饰）、电池建议（电池供电场景显性提
//!           示，建议不强制，文案可关闭）、数据不足显性（对
//!           照数据缺失标注数据不足不出量化结论）、版本化
//!           （声明随性能基线版本化）、判据。
//! 组件：声明账 + 电池建议器 + 对照数据引用。
//! 职责定位：功耗声明是「 calories 标签」——软渲费电就写费
//!           电几倍；没有对照数据就写「缺数据」不硬编数字；
//!           电池场景给提示但摁不摁由用户。
//! 性能逐项分解：声明 O(1) 常驻；判据 O(1)；引用 O(1)。
//! 诊断码段：0x32080（0x32081~0x32089 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PW_VERSION: &str = "veb350_sfpwr-v1";

/// 诊断段基址（0x32080 段 · 本模块独占）。
pub const PW_SEG_BASE: u32 = 0x32080;
pub const XC_NODATA: u32 = 0x32081; // 对照数据不足
pub const XC_MULT: u32 = 0x32082; // 倍数量级非法（<1 或越界）
pub const XC_BASEV: u32 = 0x32083; // 基线版本漂移
pub const XC_NOCLOSE: u32 = 0x32084; // 提示不可关闭（无障碍红线）

/// 量级倍数界（软渲 vs 硬渲典型 2~20 倍）。
pub const MULT_MIN: u32 = 1;
pub const MULT_MAX: u32 = 100;

/// 声明记录。
#[derive(Clone, Copy)]
pub struct Decl {
    /// 软渲功耗倍数（相对同负载硬渲）。
    pub mult: u32,
    /// 对照数据在案。
    pub has_data: bool,
    /// 基线版本号。
    pub basever: u32,
    /// 电池建议开。
    pub battery_tip: bool,
}

/// 声明账。
pub struct PwrDecl {
    cur: Option<Decl>,
    /// 版本历史。
    pub vers: Vec<u32>,
    /// 提示关闭计数（用户关闭权）。
    pub tips_closed: u32,
}

impl PwrDecl {
    /// 空账。
    pub fn new() -> PwrDecl {
        PwrDecl {
            cur: None,
            vers: Vec::new(),
            tips_closed: 0,
        }
    }

    /// 出声明：倍数夹界、对照数据缺 → 允许出声明但标注「数据不足」。
    pub fn declare(&mut self, mult: u32, has_data: bool, basever: u32) -> Result<Decl, u32> {
        if mult < MULT_MIN || mult > MULT_MAX {
            return Err(XC_MULT);
        }
        if basever == 0 {
            return Err(XC_BASEV);
        }
        let d = Decl {
            mult,
            has_data,
            basever,
            battery_tip: true,
        };
        self.cur = Some(d);
        self.vers.push(basever);
        Ok(d)
    }

    /// 量化结论：有对照数据才给数字结论；无数据显性「不足」。
    pub fn verdict(&self) -> Result<u32, u32> {
        match self.cur {
            None => Err(XC_NODATA),
            Some(ref d) => {
                if d.has_data {
                    Ok(d.mult)
                } else {
                    Err(XC_NODATA)
                }
            }
        }
    }

    /// 电池建议：倍数 ≥5 且电池供电 → 显性提示；建议不强制（可关）。
    pub fn advise(&self, battery: bool, dismissed: bool) -> Result<bool, u32> {
        let d = match self.cur {
            None => return Err(XC_NODATA),
            Some(ref d) => d,
        };
        if dismissed {
            let _ = XC_NOCLOSE;
            return Ok(false); // 用户关闭权优先
        }
        Ok(battery && d.mult >= 5)
    }

    /// 提示关闭登记。
    pub fn close_tip(&mut self) -> u32 {
        self.tips_closed += 1;
        self.tips_closed
    }

    /// 基线版本漂移检测：基线变了声明须重出。
    pub fn drift(&self, now_basever: u32) -> bool {
        match self.cur {
            None => true,
            Some(ref d) => d.basever != now_basever,
        }
    }

    /// 版本数。
    pub fn ver_count(&self) -> usize {
        self.vers.len()
    }
}

pub fn run_veb350_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0350");
    // 族零：倍数门。
    let mut d = PwrDecl::new();
    s.add("零倍拒", matches!(d.declare(0, true, 7), Err(XC_MULT)), "<1");
    s.add("爆倍拒", matches!(d.declare(101, true, 7), Err(XC_MULT)), ">100");
    s.add("零基线拒", matches!(d.declare(3, true, 0), Err(XC_BASEV)), "无基线");
    s.add("三倍出", d.declare(3, true, 7).is_ok(), "低倍如实");
    s.add("版本一", d.ver_count() == 1, "v7");
    // 族一：量级如实。
    s.add("有数据给数", d.verdict() == Ok(3), "量化");
    s.add("五倍出", d.declare(8, true, 7).is_ok(), "典型量级");
    s.add("给八", d.verdict() == Ok(8), "不粉饰");
    // 族二：数据不足显性。
    s.add("无据声明过", d.declare(12, false, 7).is_ok(), "可出但不量化");
    s.add("无据显性拒量", d.verdict() == Err(XC_NODATA), "标注不足");
    s.add("空账拒量", PwrDecl::new().verdict() == Err(XC_NODATA), "无声明");
    // 族三：电池建议。
    let mut d2 = PwrDecl::new();
    let _ = d2.declare(8, true, 7);
    s.add("电池五倍提示", d2.advise(true, false) == Ok(true), "显性提示");
    s.add("插电不提示", d2.advise(false, false) == Ok(false), "无场景");
    s.add("低倍不提示", {
        let _ = d2.declare(3, true, 7);
        d2.advise(true, false) == Ok(false)
    }, "3<5");
    s.add("关权优先", d2.advise(true, true) == Ok(false), "建议不强制");
    s.add("关闭登记", d2.close_tip() == 1, "可关闭");
    s.add("关闭二次", d2.close_tip() == 2, "累计");
    // 族四：版本化与漂移。
    s.add("基线在 7", !d2.drift(7), "同版不漂");
    s.add("基线漂移", d2.drift(8), "升版即漂");
    s.add("空账恒漂", PwrDecl::new().drift(7), "无声明=漂");
    s.add("重出声明", d2.declare(9, true, 8).is_ok(), "v8 重出");
    s.add("版本二", d2.ver_count() == 2, "历史两笔");
    s.add("漂移清", !d2.drift(8), "同版复立");
    // 族五：界内典型值。
    s.add("二十倍可表", d2.declare(20, true, 8).is_ok(), "如实上限内");
    s.add("给二十", d2.verdict() == Ok(20), "量级诚实");
    s.add("魔数核", PW_SEG_BASE == 0x32080, "段基址");
    s
}
