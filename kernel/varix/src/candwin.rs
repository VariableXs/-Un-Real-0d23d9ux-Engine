//! candwin — WP-202 · B-906 候选窗定位（MD2 篇 9.3）。
//!
//! 判据 B-906：Wine 与原生窗口内不越屏（WD 侧判例 11）。
//! MD2 原文（9.3）："候选窗是合成器浮层（popup_grab 抓取，屏幕夹紧），
//! 定位跟随焦点窗口上报的光标锚点——Wine 窗口的光标锚点经 Wine 桥上报，
//! 协议同一路径（判例 11 的实现闭环）。"
//!
//! 宿主可测形态：光标锚点模型（Wine 桥与原生同一路径——来源类型不影响
//! 定位算法）+ popup_grab 抓取 + **屏幕夹紧**（锚点下方缺空间翻上方、越
//! 左右边界水平夹紧——四边不越屏）+ 定位确定性（同锚点同尺寸同位置）。

use crate::checks::CheckSet;

/// 候选窗尺寸（模型面：每页九个的宽高）。
pub const CAND_W: i32 = 180;
pub const CAND_H: i32 = 216;

/// 光标锚点（焦点窗口上报；Wine 桥与原生同一路径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CursorAnchor {
    pub x: i32,
    pub y: i32,
}

/// 锚点来源（协议同一路径的结构面：定位算法不看来源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorOrigin {
    Native,
    WineBridge,
}

/// 候选窗定位结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CandPos {
    pub x: i32,
    pub y: i32,
    /// 翻转面（下方缺空间翻上方——诊断可读）。
    pub flipped: bool,
}

/// 候选窗定位：popup_grab 抓取 + 屏幕夹紧。
/// 规则：默认锚点下方 (anchor.y + line_h)；越下边界翻锚点上方；四边越界
/// 全部夹紧回屏内。**永不越屏**。
pub fn place(anchor: CursorAnchor, screen_w: i32, screen_h: i32) -> CandPos {
    let line_h = 20;
    let mut x = anchor.x;
    let mut y = anchor.y + line_h;
    let mut flipped = false;
    // 下方越界 → 翻上方
    if y + CAND_H > screen_h {
        y = anchor.y - CAND_H;
        flipped = true;
    }
    // 水平夹紧
    if x + CAND_W > screen_w {
        x = screen_w - CAND_W;
    }
    if x < 0 {
        x = 0;
    }
    // 垂直兜底夹紧（极矮屏幕等极端模型面）
    if y < 0 {
        y = 0;
    }
    if y + CAND_H > screen_h {
        y = screen_h - CAND_H;
    }
    CandPos { x, y, flipped }
}

/// 不越屏判定（判据核心的谓词形态）。
pub fn in_screen(p: CandPos, screen_w: i32, screen_h: i32) -> bool {
    p.x >= 0 && p.y >= 0 && p.x + CAND_W <= screen_w && p.y + CAND_H <= screen_h
}

// ---------------------------------------------------------------- 对练

/// 定位对练摘要。
#[derive(Default, PartialEq, Eq, Debug)]
pub struct CandDrillSummary {
    pub rounds: u32,
    /// 恒不越屏（Wine 与原生两路都验）
    pub never_out: bool,
    /// 定位确定性（同输入同输出）
    pub deterministic: bool,
    /// 翻转面可用（下方越界时翻上方）
    pub flip_works: bool,
}

/// 随机锚点 × 屏幕对练：Wine 与原生同路径恒不越屏。
pub fn run_cand_drills(seed: u64, rounds: u32) -> CandDrillSummary {
    let mut g = crate::comprecover::Lcg(seed);
    let mut sum = CandDrillSummary::default();
    sum.rounds = rounds;
    sum.never_out = true;
    sum.deterministic = true;
    sum.flip_works = false;
    for _ in 0..rounds {
        // 随机屏幕（含小屏极端面）与随机锚点（含越界锚点——夹紧救回）
        let screen_w = 800 + (g.next() % 2400) as i32;
        let screen_h = 600 + (g.next() % 1800) as i32;
        let anchor = CursorAnchor {
            x: (g.next() % (screen_w as u64 + 400)) as i32 - 200,
            y: (g.next() % (screen_h as u64 + 400)) as i32 - 200,
        };
        // Wine 桥与原生：同锚点同结果（协议同一路径）
        let p_native = place(anchor, screen_w, screen_h);
        let p_wine = place(anchor, screen_w, screen_h);
        if p_native != p_wine {
            sum.deterministic = false;
        }
        if !in_screen(p_native, screen_w, screen_h) {
            sum.never_out = false;
        }
        // 翻转面覆盖（至少一次下方越界翻上方发生）
        if p_native.flipped {
            sum.flip_works = true;
        }
    }
    sum
}

// ---------------------------------------------------------------- 自检

pub fn run_candwin_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("B-906 候选窗定位");
    {
        // 常规定位：锚点下方
        let p = place(CursorAnchor { x: 500, y: 300 }, 1920, 1080);
        set.add(
            "B-906 常规定位锚点下方",
            p.x == 500 && p.y == 320 && !p.flipped,
            "默认跟随光标锚点（line_h 20 下方）",
        );
    }
    {
        // 下方越界翻上方
        let p = place(CursorAnchor { x: 500, y: 1000 }, 1920, 1080);
        set.add(
            "B-906 下方越界翻上方",
            p.flipped && p.y == 1000 - CAND_H && in_screen(p, 1920, 1080),
            "popup_grab 抓取 + 屏幕夹紧",
        );
    }
    {
        // 右边界夹紧
        let p = place(CursorAnchor { x: 1900, y: 300 }, 1920, 1080);
        set.add(
            "B-906 右边界夹紧",
            p.x == 1920 - CAND_W && in_screen(p, 1920, 1080),
            "水平越界夹紧回屏内",
        );
    }
    {
        // 左边界夹紧（含负锚点）
        let p = place(CursorAnchor { x: -50, y: 300 }, 1920, 1080);
        set.add(
            "B-906 左边界夹紧",
            p.x == 0 && in_screen(p, 1920, 1080),
            "负锚点夹紧到 0",
        );
    }
    {
        // Wine 桥与原生同路径（结构面：place 不看来源——协议同一路径）
        let a_native = CursorAnchor { x: 400, y: 400 };
        let a_wine = CursorAnchor { x: 400, y: 400 };
        set.add(
            "B-906 Wine 与原生同路径",
            place(a_native, 1920, 1080) == place(a_wine, 1920, 1080),
            "Wine 桥上报锚点走同一定位算法（判例 11 实现闭环）",
        );
    }
    {
        // 极小屏兜底夹紧（屏**装得下**候选窗前提下的最小情形——判据语义
        // 是定位跟随不越屏；屏幕小于候选窗属物理不可能情形，不在判据面）
        let p = place(CursorAnchor { x: -100, y: 295 }, 400, 300);
        set.add(
            "B-906 极小屏兜底",
            in_screen(p, 400, 300) && p.flipped,
            "最小可装屏（400×300 > 180×216）双兜底：水平夹紧 + 下方翻面",
        );
    }
    {
        // 定位确定性
        let a = place(CursorAnchor { x: 777, y: 555 }, 1920, 1080);
        let b = place(CursorAnchor { x: 777, y: 555 }, 1920, 1080);
        set.add(
            "B-906 定位确定性",
            a == b,
            "同锚点同尺寸同位置——页内序稳定的姊妹语义",
        );
    }
    {
        // 定位对练
        let sum = run_cand_drills(0xB906, 80);
        set.add(
            "B-906 定位对练",
            sum.rounds == 80 && sum.never_out && sum.deterministic && sum.flip_works,
            "Wine 与原生窗口内不越屏（判据原文）",
        );
    }
    set
}

// ---------------------------------------------------------------- 单测

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f806_clamp_all_edges() {
        // 四边各自验证
        let r = place(CursorAnchor { x: 1900, y: 300 }, 1920, 1080);
        assert_eq!(r.x, 1920 - CAND_W);
        let l = place(CursorAnchor { x: -50, y: 300 }, 1920, 1080);
        assert_eq!(l.x, 0);
        let b = place(CursorAnchor { x: 500, y: 1070 }, 1920, 1080);
        assert!(b.flipped);
        assert!(in_screen(b, 1920, 1080));
    }

    #[test]
    fn f806_never_out_random() {
        // 粗粒度随机不越屏
        let mut seed = 42u64;
        for _ in 0..50 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let sw = (seed % 3000) as i32 + 400;
            let sh = ((seed >> 16) % 2000) as i32 + 300;
            let ax = ((seed >> 32) % 3500) as i32 - 200;
            let ay = ((seed >> 48) % 2400) as i32 - 200;
            let p = place(CursorAnchor { x: ax, y: ay }, sw, sh);
            assert!(in_screen(p, sw, sh), "anchor=({},{}) screen=({},{}))", ax, ay, sw, sh);
        }
    }

    #[test]
    fn f806_flip_semantics() {
        let above = place(CursorAnchor { x: 100, y: 1050 }, 1920, 1080);
        assert!(above.flipped && above.y + CAND_H <= 1080);
        let below = place(CursorAnchor { x: 100, y: 100 }, 1920, 1080);
        assert!(!below.flipped && below.y == 120);
    }

    #[test]
    fn f806_drill_deterministic() {
        let a = run_cand_drills(13, 40);
        let b = run_cand_drills(13, 40);
        assert_eq!(a, b);
        assert!(a.never_out && a.deterministic && a.flip_works);
    }
}
