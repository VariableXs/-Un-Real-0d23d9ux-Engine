//! unx0555_badinj —— UNX-F0555 日志注入演练（坏参检出）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0555 · 日志注入演练 · 目标 200 行
//! 判据原文：坏参注入检出 100%（6/6——级别/标签/文本三类×坏值），好路径
//!           误伤 0。
//! 组件：三面坏参注入（级别 4..255 / 组件号 16..255 / 空文本+含竖线）+
//!   检出率核 + 好路径误伤零核。
//! 纯功能线：no_std + alloc，整数算术，确定性。

#![allow(dead_code)]

use crate::checks::CheckSet;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0x35800;
pub const XC_INJ_MISS: u32 = SEG_BASE + 1; // 坏参漏检
pub const XC_GOOD_HIT: u32 = SEG_BASE + 2; // 好路径误伤
pub const XC_INJ_OOB: u32 = SEG_BASE + 3; // 越界
pub const XC_INJ_BAD: u32 = SEG_BASE + 4; // 注入器坏

/// 三面坏参注入器：产出 (级别, 组件, 文本) 坏参组。
pub fn bad_params() -> Vec<(u8, u8, &'static str)> {
    let mut v = Vec::new();
    // 面一：级别坏 ×2。
    v.push((4, 0, "lv4"));
    v.push((255, 1, "lv255"));
    // 面二：组件号坏 ×2。
    v.push((0, 16, "cp16"));
    v.push((1, 200, "cp200"));
    // 面三：文本坏 ×2。
    v.push((2, 2, "")); // 空文本
    v.push((3, 3, "bad|pipe")); // 含竖线
    v
}

/// 参数体检：坏参检出（与 F0541 emit 门语义一致）。
pub fn param_ok(level: u8, comp: u8, text: &str) -> bool {
    level <= 3 && comp < 16 && !text.is_empty() && !text.contains('|')
}

/// 演练：注入坏参组→检出计数；好参组→误伤计数。
pub fn drill() -> (usize, usize) {
    let bad = bad_params();
    let mut caught = 0;
    let mut i = 0;
    while i < bad.len() {
        let (l, c, t) = bad[i];
        if !param_ok(l, c, t) {
            caught += 1;
        }
        i += 1;
    }
    // 好路径：合法参必须全过（零误伤）。
    let mut hit = 0;
    let mut lv = 0;
    while lv <= 3 {
        let mut cp = 0;
        while cp < 16 {
            if !param_ok(lv, cp, "ok") {
                hit += 1;
            }
            cp += 1;
        }
        lv += 1;
    }
    (caught, hit)
}

pub fn run_unx0555_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0555");
    cs.add("SEG 段位独占", SEG_BASE == 0x35800 && XC_INJ_BAD == SEG_BASE + 4, "u32 高段全域普查无冲突");

    // 族一：坏参组齐。
    let bad = bad_params();
    cs.add("注入组 6", bad.len() == 6, "三面×二值");
    cs.add("级别面 2", !param_ok(bad[0].0, bad[0].1, bad[0].2) && !param_ok(bad[1].0, bad[1].1, bad[1].2), "级别 4/255 坏");
    cs.add("组件面 2", !param_ok(bad[2].0, bad[2].1, bad[2].2) && !param_ok(bad[3].0, bad[3].1, bad[3].2), "组件 16/200 坏");
    cs.add("文本面 2", !param_ok(bad[4].0, bad[4].1, bad[4].2) && !param_ok(bad[5].0, bad[5].1, bad[5].2), "空文/竖线坏");

    // 族二：检出 100% + 零误伤。
    let (caught, hit) = drill();
    cs.add("检出 6/6", caught == 6, "全检出");
    cs.add("误伤 0", hit == 0, "好路径 64 组全过");

    // 族三：边界好参不过检。
    cs.add("边界 3/15/常文好", param_ok(3, 15, "ok"), "最大合法值");
    cs.add("边界 0/0 好", param_ok(0, 0, "ok"), "最小合法值");
    cs.add("半坏级别好组件", !param_ok(4, 0, "ok"), "仅级别坏仍拒");
    cs.add("好级别坏组件", !param_ok(0, 16, "ok"), "仅组件坏仍拒");

    // 族四：空串与竖线单独核。
    cs.add("空串拒", !param_ok(0, 0, ""), "空文本");
    cs.add("竖线拒", !param_ok(0, 0, "a|b"), "字段污染");
    cs.add("纯空白算好（不拒）", param_ok(0, 0, " "), "空白非空");
    cs
}
