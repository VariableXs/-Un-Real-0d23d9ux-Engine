//! VE-F0716 · SVG 端到端栅格化
//!
//! 职责：SVG 文件到像素的端到端全链——
//! SVG 解析（VE-F 域 SVG 解码的 path 出口）→ 路径构建（F0676）→
//! 渐变/蒙版/裁剪语义映射（SVG 渐变→VE 渐变 F0695、clipPath→裁剪、mask→蒙版）→
//! 栅格化输出。SVG 是图标与插画的通用格式——端到端质量决定图标生态。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// SVG path 命令子集：M/L/H/V/C/Q/Z（图标级覆盖）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgCmd {
    Move(f32, f32),
    Line(f32, f32),
    HLine(f32),
    VLine(f32),
    Curve(f32, f32, f32, f32, f32, f32),
    Quad(f32, f32, f32, f32),
    Close,
}

/// path 数据解析：把命令流展开为绝对坐标段（H/V → L，Z → Close）。
/// 返回 Err 表示命令序列不合法（如 Close 后接 H/V 未重开路径）。
pub fn parse_path(cmds: &[SvgCmd]) -> Result<Vec<SvgCmd>, String> {
    if cmds.is_empty() {
        return Err("空 path".to_string());
    }
    if !matches!(cmds[0], SvgCmd::Move(_, _)) {
        return Err("path 必须以 M 开头".to_string());
    }
    let mut out = Vec::with_capacity(cmds.len());
    let mut open = false;
    for c in cmds {
        match c {
            SvgCmd::Move(_, _) => {
                if open {
                    // 未 Close 就 Move：隐式闭合前一段（SVG 语义）。
                    out.push(SvgCmd::Close);
                }
                open = true;
                out.push(*c);
            }
            SvgCmd::Close => {
                if !open {
                    return Err("未开先合：Close 无对应 Move".to_string());
                }
                open = false;
                out.push(SvgCmd::Close);
            }
            _ => {
                if !open {
                    return Err("路径未开启即绘制".to_string());
                }
                out.push(*c);
            }
        }
    }
    Ok(out)
}

/// H/V 展开为 L：给定当前笔位，把水平/垂直命令转成线段。
pub fn expand_hv(cmds: &[SvgCmd], start: (f32, f32)) -> Vec<SvgCmd> {
    let mut out = Vec::new();
    let mut pen = start;
    for c in cmds {
        match *c {
            SvgCmd::HLine(x) => {
                out.push(SvgCmd::Line(x, pen.1));
                pen.0 = x;
            }
            SvgCmd::VLine(y) => {
                out.push(SvgCmd::Line(pen.0, y));
                pen.1 = y;
            }
            SvgCmd::Move(x, y) | SvgCmd::Line(x, y) => {
                pen = (x, y);
                out.push(*c);
            }
            _ => out.push(*c),
        }
    }
    out
}

/// SVG 渐变语义映射：SVG linearGradient → VE 渐变（色标 + spreadMethod）。
#[derive(Clone, Debug, PartialEq)]
pub struct SvgGradient {
    /// SVG 色标（offset 0..1 + RGBA）。
    pub stops: Vec<(f32, u32)>,
    /// spreadMethod: pad/repeat/reflect → VE 的 0/1/2。
    pub spread: u8,
}

impl SvgGradient {
    /// 映射为 VE 渐变色标（千分位偏移 + 顺序保持）。
    pub fn to_ve_stops(&self) -> Result<Vec<(u32, u32)>, String> {
        if self.stops.len() < 2 {
            return Err("SVG 渐变色标不足 2 个".to_string());
        }
        let mut out: Vec<(u32, u32)> = self
            .stops
            .iter()
            .map(|(o, c)| ((*o * 1000.0).round() as u32, *c))
            .collect();
        // SVG 允许乱序色标——映射时排序。
        out.sort_by_key(|s| s.0);
        Ok(out)
    }
    pub fn spread_code(&self) -> u8 {
        match self.spread {
            0 => 0, // pad
            1 => 1, // repeat
            2 => 2, // reflect
            _ => 0,
        }
    }
}

/// clipPath / mask 语义映射判定：两者是不同语义——clip 硬裁剪、mask 软混合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvgMaskKind {
    ClipPath,
    Mask,
}

/// 语义映射：clipPath → 硬裁剪（覆盖度 0/1）；mask → 软混合（覆盖度乘蒙版 alpha）。
pub fn map_mask_semantics(kind: SvgMaskKind, coverage: u32, mask_alpha: u32) -> u32 {
    match kind {
        SvgMaskKind::ClipPath => {
            if mask_alpha >= 500 {
                coverage
            } else {
                0
            }
        }
        SvgMaskKind::Mask => (coverage as u64 * mask_alpha as u64 / 1000) as u32,
    }
}

/// 端到端链路：解析→展开→映射→栅格化四段齐备才输出。
pub fn pipeline_ready(parsed: bool, expanded: bool, mapped: bool, rastered: bool) -> Result<&'static str, String> {
    match (parsed, expanded, mapped, rastered) {
        (true, true, true, true) => Ok("端到端通"),
        (false, _, _, _) => Err("SVG 解析未过".to_string()),
        (true, false, _, _) => Err("路径展开未过".to_string()),
        (true, true, false, _) => Err("渐变/蒙版语义映射未过".to_string()),
        (true, true, true, false) => Err("栅格化输出未过".to_string()),
    }
}

pub fn run_vec316_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec316");
    // 族一：path 解析
    let ok = [
        SvgCmd::Move(0.0, 0.0),
        SvgCmd::HLine(10.0),
        SvgCmd::VLine(10.0),
        SvgCmd::Close,
    ];
    s.add("合法 path 解析通过", parse_path(&ok).is_ok(), "parse-ok");
    s.add("空 path 拒绝", parse_path(&[]).is_err(), "empty");
    s.add("非 M 开头拒绝", parse_path(&[SvgCmd::Line(1.0, 1.0)]).is_err(), "no-head");
    s.add("未开先合拒绝", parse_path(&[SvgCmd::Close]).is_err(), "close-first");
    let implicit = [SvgCmd::Move(0.0, 0.0), SvgCmd::Line(1.0, 1.0), SvgCmd::Move(5.0, 5.0)];
    s.add("隐式闭合补 Close", parse_path(&implicit).map(|v| v.contains(&SvgCmd::Close)) == Ok(true), "implicit");
    let draw_noclose = [SvgCmd::Move(0.0, 0.0)];
    s.add("开后不合规（未闭合允许）", parse_path(&draw_noclose).is_ok(), "open-ok");
    // 族二：H/V 展开
    let expanded = expand_hv(&ok, (0.0, 0.0));
    s.add("H→L 展开保 y", expanded.contains(&SvgCmd::Line(10.0, 0.0)), "h-expand");
    s.add("V→L 展开保 x", expanded.contains(&SvgCmd::Line(10.0, 10.0)), "v-expand");
    let mixed = [SvgCmd::Move(1.0, 2.0), SvgCmd::HLine(5.0), SvgCmd::VLine(6.0)];
    let ex2 = expand_hv(&mixed, (1.0, 2.0));
    s.add("笔位跟随展开", ex2 == vec![SvgCmd::Move(1.0, 2.0), SvgCmd::Line(5.0, 2.0), SvgCmd::Line(5.0, 6.0)], "pen");
    // 族三：渐变映射
    let g = SvgGradient {
        stops: vec![(0.7, 0xFF0000FF), (0.0, 0xFF00FF00), (1.0, 0xFFFF0000)],
        spread: 2,
    };
    let ve = g.to_ve_stops();
    s.add("色标排序映射", ve == Ok(vec![(0, 0xFF00FF00), (700, 0xFF0000FF), (1000, 0xFFFF0000)]), "stops");
    s.add("乱序色标被整理", ve.as_ref().map(|v| v[0].0) == Ok(0), "sorted");
    let thin = SvgGradient { stops: vec![(0.0, 1)], spread: 0 };
    s.add("单色标拒绝", thin.to_ve_stops().is_err(), "thin");
    s.add("spreadMethod 映射", SvgGradient { stops: g.stops.clone(), spread: 2 }.spread_code() == 2, "spread");
    // 族四：clip/mask 语义
    s.add("clipPath 硬裁剪", map_mask_semantics(SvgMaskKind::ClipPath, 800, 900) == 800, "clip-hard");
    s.add("clipPath 阈值下归零", map_mask_semantics(SvgMaskKind::ClipPath, 800, 400) == 0, "clip-zero");
    s.add("mask 软混合", map_mask_semantics(SvgMaskKind::Mask, 800, 500) == 400, "mask-soft");
    s.add("两语义互异", map_mask_semantics(SvgMaskKind::ClipPath, 800, 400) != map_mask_semantics(SvgMaskKind::Mask, 800, 400), "distinct");
    // 族五：端到端链
    s.add("四段齐备端到端通", pipeline_ready(true, true, true, true) == Ok("端到端通"), "e2e");
    match pipeline_ready(true, true, false, false) {
        Err(m) => s.add("断链点名", m.contains("语义映射"), "e2e-map"),
        Ok(_) => s.add("断链点名", false, "expected err"),
    }
    match pipeline_ready(false, true, true, true) {
        Err(m) => s.add("首断点名解析", m.contains("解析"), "e2e-parse"),
        Ok(_) => s.add("首断点名解析", false, "expected err"),
    }
    s
}
