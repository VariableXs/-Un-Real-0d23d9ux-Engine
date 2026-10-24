//! VE-F0762 九宫格角边中心策略
//! 职责：每区绘制策略枚举（Blit/StretchH/StretchV/StretchBoth）、
//!       策略推导（区位置 → 策略）、UV 映射（屏幕区 → 纹理子区）、
//!       策略与 UV 的一致性校验。
//! 契约：角策略恒 Blit；上/下边 StretchH；左/右边 StretchV；中心 StretchBoth。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Slice, layout};

/// 绘制策略
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DrawStrategy {
    Blit,
    StretchH,
    StretchV,
    StretchBoth,
}

/// 区序号（0..9，行主序）
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Zone {
    TL,
    Top,
    TR,
    Left,
    Center,
    Right,
    BL,
    Bottom,
    BR,
}

pub const ZONES: [Zone; 9] = [
    Zone::TL, Zone::Top, Zone::TR,
    Zone::Left, Zone::Center, Zone::Right,
    Zone::BL, Zone::Bottom, Zone::BR,
];

/// 区 → 策略（规范推导）
pub fn strategy_of(z: Zone) -> DrawStrategy {
    match z {
        Zone::TL | Zone::TR | Zone::BL | Zone::BR => DrawStrategy::Blit,
        Zone::Top | Zone::Bottom => DrawStrategy::StretchH,
        Zone::Left | Zone::Right => DrawStrategy::StretchV,
        Zone::Center => DrawStrategy::StretchBoth,
    }
}

/// UV 子区映射：屏幕区 → 纹理空间矩形
pub fn uv_of(s: &Slice, zone: Zone) -> (f64, f64, f64, f64) {
    let (l, t, r, b) = (s.inset.left, s.inset.top, s.inset.right, s.inset.bottom);
    let cw = s.center_w();
    let ch = s.center_h();
    match zone {
        Zone::TL => (0.0, 0.0, l, t),
        Zone::Top => (l, 0.0, cw, t),
        Zone::TR => (l + cw, 0.0, r, t),
        Zone::Left => (0.0, t, l, ch),
        Zone::Center => (l, t, cw, ch),
        Zone::Right => (l + cw, t, r, ch),
        Zone::BL => (0.0, t + ch, l, b),
        Zone::Bottom => (l, t + ch, cw, b),
        Zone::BR => (l + cw, t + ch, r, b),
    }
}

/// UV 子区面积和 = 纹理面积
pub fn uv_covers(s: &Slice) -> bool {
    let total: f64 = ZONES.iter().map(|&z| { let (_, _, w, h) = uv_of(s, z); w * h }).sum();
    (total - s.tex_w * s.tex_h).abs() < 1e-9
}

/// 屏幕区与 UV 区的纵横比一致性（Blit 区必须同比；拉伸区无此约束）
pub fn blit_ratio_ok(s: &Slice, cells: &[Cell; 9]) -> bool {
    ZONES.iter().zip(cells.iter()).all(|(&z, c)| {
        if strategy_of(z) != DrawStrategy::Blit {
            return true;
        }
        let (_, _, uw, uh) = uv_of(s, z);
        if uh == 0.0 {
            return c.h == 0.0;
        }
        (c.w / c.h - uw / uh).abs() < 1e-6 || (uh / c.h - 1.0).abs() < 1e-6
    })
}

pub fn run_vec362_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec362");
    let ins = super::vec361_ninecore::Insets { left: 8.0, top: 8.0, right: 8.0, bottom: 8.0 };
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    // 族一：策略推导
    s.add("策略-角Blit", strategy_of(Zone::TL) == DrawStrategy::Blit && strategy_of(Zone::BR) == DrawStrategy::Blit, "corners blit");
    s.add("策略-边StretchH", strategy_of(Zone::Top) == DrawStrategy::StretchH && strategy_of(Zone::Bottom) == DrawStrategy::StretchH, "h edges");
    s.add("策略-边StretchV", strategy_of(Zone::Left) == DrawStrategy::StretchV && strategy_of(Zone::Right) == DrawStrategy::StretchV, "v edges");
    s.add("策略-中心Both", strategy_of(Zone::Center) == DrawStrategy::StretchBoth, "center");
    s.add("策略-九区全", ZONES.len() == 9, "9 zones");
    // 策略分布：4 Blit + 2 StretchH + 2 StretchV + 1 StretchBoth
    let counts = [
        ZONES.iter().filter(|&&z| strategy_of(z) == DrawStrategy::Blit).count(),
        ZONES.iter().filter(|&&z| strategy_of(z) == DrawStrategy::StretchH).count(),
        ZONES.iter().filter(|&&z| strategy_of(z) == DrawStrategy::StretchV).count(),
        ZONES.iter().filter(|&&z| strategy_of(z) == DrawStrategy::StretchBoth).count(),
    ];
    s.add("策略-分布", counts == [4, 2, 2, 1], "4/2/2/1");
    // 族二：UV 映射
    let (ux, uy, uw, uh) = uv_of(&sl, Zone::TL);
    s.add("UV-左上", ux == 0.0 && uy == 0.0 && uw == 8.0 && uh == 8.0, "TL uv");
    let (ux2, _, uw2, _) = uv_of(&sl, Zone::Top);
    s.add("UV-上边", ux2 == 8.0 && uw2 == 48.0, "top uv");
    let (_, _, uw3, uh3) = uv_of(&sl, Zone::Center);
    s.add("UV-中心", uw3 == 48.0 && uh3 == 48.0, "center uv");
    let (ux4, uy4, _, _) = uv_of(&sl, Zone::BR);
    s.add("UV-右下", ux4 == 56.0 && uy4 == 56.0, "BR origin");
    // UV 覆盖
    s.add("UV-全覆盖", uv_covers(&sl), "uv area sum");
    // 非对称 UV
    let asym = Slice::build(100.0, 100.0, super::vec361_ninecore::Insets { left: 10.0, top: 20.0, right: 30.0, bottom: 5.0 }).expect("a");
    let (ax, ay, aw, ah) = uv_of(&asym, Zone::Center);
    s.add("UV-非对称中心", ax == 10.0 && ay == 20.0 && aw == 60.0 && ah == 75.0, "asym center");
    s.add("UV-非对称覆盖", uv_covers(&asym), "asym covers");
    // 族三：Blit 比例一致
    let cells = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("cells");
    s.add("比例-Blit一致", blit_ratio_ok(&sl, &cells), "corners 1:1");
    // 非均匀缩放的 Blit 破坏比例（角区仍 8×8 与 uv 同——恒真；用非方角验证）
    let rect_slice = Slice::build(40.0, 40.0, super::vec361_ninecore::Insets { left: 8.0, top: 4.0, right: 8.0, bottom: 4.0 }).expect("rs");
    let rc = layout(&rect_slice, 0.0, 0.0, 200.0, 100.0).expect("rc");
    s.add("比例-矩形角", blit_ratio_ok(&rect_slice, &rc), "8:4 == 8:4");
    // 族四：策略×UV 对应（拉伸区 UV 尺寸与屏幕尺寸解耦）
    s.add("解耦-上边", {
        let (_, _, uw, uh) = uv_of(&sl, Zone::Top);
        let c = cells[1];
        // UV 高 8 固定，屏高 8 固定；UV 宽 48，屏宽 84 伸缩
        uw == 48.0 && c.w == 84.0 && uh == c.h
    }, "top decoupled w");
    s.add("解耦-中心", {
        let (_, _, uw, uh) = uv_of(&sl, Zone::Center);
        let c = cells[4];
        uw == 48.0 && uh == 48.0 && c.w == 84.0 && c.h == 64.0
    }, "center both stretch");
    // 族五：角区屏幕尺寸 == UV 尺寸
    s.add("角-屏幕等于UV", {
        let (_, _, uw, uh) = uv_of(&sl, Zone::TL);
        let c = cells[0];
        uw == c.w && uh == c.h
    }, "TL equal");
    s
}
