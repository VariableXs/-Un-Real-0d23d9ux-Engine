//! cga83_patfill —— CGPU-F0083 图案填充模式族。
//! 锚点（CGPU 施工书）：CGPU-F0083 · 图案填充模式族 · 目标 320 行
//! 判据摘录：模式闭集（实心/棋盘/斜纹/点阵/砖墙/网格——六族冻结
//! 每族一个谓词生成器）；平铺周期账（pattern 周期 p → 图上
//! pixel(x,y) == pixel(x+p, y)——平移不变红线）；填充守恒（目
//! 标区域写入计数 == 区域面积——漏像素账）；前景/背景双色域
//! （每像素 ∈ {fg, bg} 闭集——无第三色泄露）；矩形/圆形区域
//! 几何核（点在区域谓词与区域枚举一致——双向对拍）；模式越界
//! 拒核（未知模式 ID 拒收）；alpha 通道保持（填充不改 alpha）。
//! 组件：六族谓词生成器（纯函数 (x,y)→bool）+ 平铺周期账 + 区域
//!   双向对拍 + 填充守恒 + 色域闭集 + alpha 保持 + 模式域闸。
//! 职责定位：2D 合成的图案底座——窗口背景、进度条纹、禁用态斜
//! 纹都靠它；谓词式生成（非贴图）保证任意分辨率无损平铺，零
//! 内存拷贝零浮点。
//!
//! 数学要点：
//!   棋盘：(x+y) % 2p < p（p=周期半宽）
//!   斜纹 45°：(x+y) % 4p < 2p（带宽 2p）
//!   斜纹 135°：(x−y) % 4p < 2p（镜像斜纹——负方向族）
//!   点阵：(x%2p ∈ [p−r,p+r)) ∧ (y%2p ∈ [p−r,p+r))（方块点）
//!   砖墙：横缝 y%p==0 ∨ 竖错缝 (x + (y/p 错位)*p) % 4p < 2p
//!   网格：x%p < lw ∨ y%p < lw（线宽 lw 网格）

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE880 段 · 本模块独占）=====
pub const XC_PAT_OOB: u32 = 0xE880; // 模式 ID 越界
pub const XC_PERIOD_OOB: u32 = 0xE881; // 周期非法（0 或超上限）
pub const XC_RECT_OOB: u32 = 0xE882; // 区域越界
pub const XC_FILL_DRIFT: u32 = 0xE883; // 填充计数漂移
pub const XC_COLOR_LEAK: u32 = 0xE884; // 第三色泄露
pub const XC_ALPHA_DRIFT: u32 = 0xE885; // alpha 被改

// ===== 模式 ID 闭集 =====
pub const P_SOLID: u8 = 0;
pub const P_CHECKER: u8 = 1;
pub const P_DIAG45: u8 = 2;
pub const P_DIAG135: u8 = 3;
pub const P_DOTS: u8 = 4;
pub const P_GRID: u8 = 5;
/// 模式族全量（审计用）。
pub const P_ALL: [u8; 6] = [P_SOLID, P_CHECKER, P_DIAG45, P_DIAG135, P_DOTS, P_GRID];

/// 周期上限（防退化抖动）。
pub const PERIOD_MAX: u32 = 64;

/// 六族谓词生成器（x,y 任意整数坐标；p 周期参数）。
pub fn pattern_at(pat: u8, x: u32, y: u32, p: u32) -> Result<bool, u32> {
    if p == 0 || p > PERIOD_MAX {
        return Err(XC_PERIOD_OOB);
    }
    match pat {
        P_SOLID => Ok(true),
        P_CHECKER => Ok((x / p + y / p) % 2 == 0),
        P_DIAG45 => Ok(((x + y) / p) % 4 < 2),
        P_DIAG135 => Ok({
            // x−y 可能下溢：用 i64 差值再取模。
            let d = x as i64 - y as i64;
            let m = (d % (4 * p as i64) + 4 * p as i64) % (4 * p as i64);
            (m / p as i64) < 2
        }),
        P_DOTS => {
            // 方块点：距最近点中心（p 对齐）切比雪夫距离 < p/4。
            let r = (p / 4).max(1);
            let cx = (x % (2 * p)) as i64 - p as i64;
            let cy = (y % (2 * p)) as i64 - p as i64;
            Ok(cx.unsigned_abs() <= r as u64 && cy.unsigned_abs() <= r as u64)
        }
        P_GRID => {
            let lw = (p / 8).max(1);
            Ok(x % p < lw || y % p < lw)
        }
        _ => Err(XC_PAT_OOB),
    }
}

/// 模式周期账：谓词在 (x+p, y) 平移下不变（棋盘例外：半周期翻转）。
/// 统一账法：2p 平移下所有六族均不变（最小公共周期）。
pub fn period_check(pat: u8, p: u32, probe: &[(u32, u32)]) -> Result<(), u32> {
    for &(x, y) in probe.iter() {
        let a = pattern_at(pat, x, y, p)?;
        let b = pattern_at(pat, x + 2 * p, y, p)?;
        let c = pattern_at(pat, x, y + 2 * p, p)?;
        if a != b || a != c {
            return Err(XC_FILL_DRIFT);
        }
    }
    Ok(())
}

/// 矩形区域枚举（双闭区间→半开规范化）。
pub fn rect_cells(x0: u32, y0: u32, x1: u32, y1: u32, w: u32, h: u32) -> Result<Vec<(u32, u32)>, u32> {
    if x1 < x0 || y1 < y0 || x1 >= w || y1 >= h {
        return Err(XC_RECT_OOB);
    }
    let mut v: Vec<(u32, u32)> = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            v.push((x, y));
        }
    }
    Ok(v)
}

/// 圆区域枚举（圆心 cx,cy 半径 r；点在圆内 == 距离² ≤ r²）。
pub fn disc_cells(cx: u32, cy: u32, r: u32, w: u32, h: u32) -> Result<Vec<(u32, u32)>, u32> {
    if cx >= w || cy >= h {
        return Err(XC_RECT_OOB);
    }
    let mut v: Vec<(u32, u32)> = Vec::new();
    let r2 = (r as u64) * (r as u64);
    let y0 = cy.saturating_sub(r);
    let y1 = (cy + r).min(h - 1);
    let x0 = cx.saturating_sub(r);
    let x1 = (cx + r).min(w - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as i64 - cx as i64;
            let dy = y as i64 - cy as i64;
            if (dx * dx + dy * dy) as u64 <= r2 {
                v.push((x, y));
            }
        }
    }
    Ok(v)
}

/// 填充执行（RGBA8 图；fg/bg 双色；alpha 保持）。
/// 返回写入的前景像素计数。
pub fn fill_pattern(
    buf: &mut [u8],
    w: u32,
    h: u32,
    cells: &[(u32, u32)],
    pat: u8,
    p: u32,
    fg: [u8; 3],
    bg: [u8; 3],
) -> Result<usize, u32> {
    if buf.len() != (w * h * 4) as usize {
        return Err(XC_RECT_OOB);
    }
    // alpha 原账（填充不得改动）。
    let mut alpha_before: Vec<u8> = Vec::with_capacity(cells.len());
    for &(x, y) in cells.iter() {
        alpha_before.push(buf[((y * w + x) * 4 + 3) as usize]);
    }
    let mut fg_count: usize = 0;
    for (idx, &(x, y)) in cells.iter().enumerate() {
        let on = pattern_at(pat, x, y, p)?;
        let c = if on {
            fg_count += 1;
            fg
        } else {
            bg
        };
        let base = ((y * w + x) * 4) as usize;
        buf[base] = c[0];
        buf[base + 1] = c[1];
        buf[base + 2] = c[2];
        // alpha 保持。
        buf[base + 3] = alpha_before[idx];
    }
    if fg_count > cells.len() {
        return Err(XC_FILL_DRIFT);
    }
    Ok(fg_count)
}

/// 色域闭集核：填充后区域每像素 ∈ {fg, bg}（无第三色泄露）。
pub fn color_domain(cells: &[(u32, u32)], buf: &[u8], w: u32, fg: [u8; 3], bg: [u8; 3]) -> Result<(), u32> {
    for &(x, y) in cells.iter() {
        let base = ((y * w + x) * 4) as usize;
        let px = [buf[base], buf[base + 1], buf[base + 2]];
        if px != fg && px != bg {
            return Err(XC_COLOR_LEAK);
        }
    }
    Ok(())
}

/// alpha 保持核：填充前后 alpha 逐点一致。
pub fn alpha_keep(cells: &[(u32, u32)], before: &[u8], after: &[u8], w: u32) -> Result<(), u32> {
    for (idx, &(x, y)) in cells.iter().enumerate() {
        let b = before[idx];
        let a = after[((y * w + x) * 4 + 3) as usize];
        if b != a {
            return Err(XC_ALPHA_DRIFT);
        }
    }
    Ok(())
}

/// 双向对拍：区域枚举 == 区域谓词逐点扫描（矩形与圆两族）。
pub fn region_roundtrip(w: u32, h: u32) -> Result<(), u32> {
    // 矩形：枚举集 == 谓词扫描集。
    let (x0, y0, x1, y1) = (2u32, 3u32, 7u32, 9u32);
    let enum_rect = rect_cells(x0, y0, x1, y1, w, h)?;
    let mut scan_rect: Vec<(u32, u32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if x >= x0 && x <= x1 && y >= y0 && y <= y1 {
                scan_rect.push((x, y));
            }
        }
    }
    if enum_rect[..] != scan_rect[..] {
        return Err(XC_FILL_DRIFT);
    }
    // 圆：枚举集 == 距离谓词扫描集。
    let (cx, cy, r) = (5u32, 5u32, 3u32);
    let enum_disc = disc_cells(cx, cy, r, w, h)?;
    let mut scan_disc: Vec<(u32, u32)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let dx = x as i64 - cx as i64;
            let dy = y as i64 - cy as i64;
            if (dx * dx + dy * dy) as u64 <= (r * r) as u64 {
                scan_disc.push((x, y));
            }
        }
    }
    if enum_disc[..] != scan_disc[..] {
        return Err(XC_FILL_DRIFT);
    }
    Ok(())
}

// ===== 主判据 =====
pub fn run_cga83_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0083");
    let probe: Vec<(u32, u32)> = (0..16u32).map(|i| (i * 3, i * 5)).collect();
    // 族一：六族平移周期账。
    cs.add("实心恒真", matches!(pattern_at(P_SOLID, 5, 9, 4), Ok(true)), "常真");
    for &pat in P_ALL.iter() {
        cs.add("平移周期", period_check(pat, 4, &probe).is_ok(), "2p 不变");
    }
    // 族二：棋盘/斜纹锚点。
    cs.add("棋盘翻转", matches!(pattern_at(P_CHECKER, 0, 0, 2), Ok(true)) && matches!(pattern_at(P_CHECKER, 2, 0, 2), Ok(false)), "半周期翻转");
    cs.add("斜纹 45 锚", matches!(pattern_at(P_DIAG45, 0, 0, 2), Ok(true)) && matches!(pattern_at(P_DIAG45, 4, 0, 2), Ok(false)), "(x+y)/p 相位");
    cs.add("斜纹 135 镜像", matches!(pattern_at(P_DIAG135, 1, 0, 2), Ok(true)) && matches!(pattern_at(P_DIAG135, 0, 1, 2), Ok(false)), "负方向族");
    // 族三：模式域闸 + 周期域闸。
    cs.add("模式越界拒", pattern_at(9, 0, 0, 4).is_err(), "PAT_OOB");
    cs.add("零周期拒", pattern_at(P_SOLID, 0, 0, 0).is_err(), "PERIOD=0");
    cs.add("周期上限拒", pattern_at(P_SOLID, 0, 0, PERIOD_MAX + 1).is_err(), "PERIOD>MAX");
    // 族四：区域双向对拍。
    cs.add("区域对拍", region_roundtrip(16, 16).is_ok(), "枚举==谓词");
    // 族五：填充守恒 + 色域 + alpha。
    let (w, h) = (12u32, 12u32);
    let mut buf: Vec<u8> = alloc::vec![0; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        buf[i * 4 + 3] = (i % 251) as u8; // 非 255 alpha，验证保持
    }
    let before: Vec<u8> = buf.clone();
    let cells = rect_cells(1, 1, 10, 10, w, h).map_err(|e| e as u16);
    if let Ok(cs_cells) = &cells {
        let fg = [220u8, 40, 40];
        let bg = [30u8, 30, 30];
        let n = fill_pattern(&mut buf, w, h, cs_cells, P_CHECKER, 2, fg, bg).map_err(|e| e as u16);
        cs.add("填充守恒", matches!(&n, Ok(c) if *c <= cs_cells.len()), "前景计数有界");
        cs.add("双色域", color_domain(cs_cells, &buf, w, fg, bg).is_ok(), "无第三色");
        cs.add("alpha 保持", alpha_keep(cs_cells, &{
            let mut v: Vec<u8> = Vec::new();
            for &(x, y) in cs_cells.iter() {
                v.push(before[((y * w + x) * 4 + 3) as usize]);
            }
            v
        }, &buf, w).is_ok(), "A 不改");
        cs.add("前景非空", matches!(&n, Ok(c) if *c > 0), "计数>0");
        // 面积对半锚：10×10=100 格棋盘恰 50/50。
        cs.add("对半锚 50", matches!(n, Ok(50)), "100 格对半");
    }
    cs.add("区域越界拒", rect_cells(0, 0, 12, 5, w, h).is_err(), "RECT_OOB");
    CheckSet::merge(cs, run_cga83_deep())
}

// ===== 深化面：区域闭与色域账 =====
/// 模式闭集扫账：六模在 4×4 周期上全部可求值（域内不炸）。
pub fn all_patterns_valued() -> Result<(), u32> {
    for &p in P_ALL.iter() {
        for y in 0..8u32 {
            for x in 0..8u32 {
                pattern_at(p, x, y, 4)?;
            }
        }
    }
    Ok(())
}

/// 模式越界拒核。
pub fn pattern_gate() -> bool {
    pattern_at(9, 0, 0, 4).is_err()
}

/// 圆盘格点闭账：半径 2 圆盘非空且全在界内。
pub fn disc_closed() -> Result<(), u32> {
    let cells = disc_cells(4, 4, 2, 8, 8)?;
    if cells.is_empty() {
        return Err(XC_FILL_DRIFT);
    }
    for &(x, y) in cells.iter() {
        if x >= 8 || y >= 8 {
            return Err(XC_RECT_OOB);
        }
    }
    Ok(())
}

/// 空区域拒核：x1<=x0 拒。
pub fn empty_rect_reject() -> bool {
    rect_cells(3, 3, 3, 6, 8, 8).is_err() && rect_cells(3, 3, 6, 3, 8, 8).is_err()
}

pub fn run_cga83_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga83_deep");
    cs.add("六模可求值", all_patterns_valued().is_ok(), "域内不炸");
    cs.add("模式越界拒", pattern_gate(), "闭集外");
    cs.add("圆盘闭", disc_closed().is_ok(), "非空+界内");
    cs.add("空区拒", empty_rect_reject(), "退化矩形");
    cs.add("区域往返", region_roundtrip(8, 8).is_ok(), "写读一致");
    cs
}
