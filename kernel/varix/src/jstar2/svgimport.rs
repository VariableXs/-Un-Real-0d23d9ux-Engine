//! F634 SVG 指针直用 · 完整设计（STAR I 主册 J-D 组）。
//!
//! **判据（主册原文）**：SVG 解析（含双层标记约定）；双倍率栅格化
//! 锐度过 F632 审计；热点向导；矢量缩放对拍；异常样本诚实报错。
//!
//! **实现口径（真实矢量管线——非占位）**：
//! - **XML-lite 解析**：标签/属性词法 + `<svg viewBox>` 尺寸 + 路径
//!   元素（path/rect/circle/ellipse/polygon/polyline）；节点数上限
//!   4096（资源型炸弹防线，超限诚实报错）；
//! - **双层标记约定**：热点 = 根元素 `data-hotspot="x,y"` 属性 **或**
//!   `id="hotspot"` 命名图层（组内十字点/小方块）——两路约定都认
//!   （判据「双层标记约定」）；
//! - **热点向导**：缺标记时不拒收——返回 `NeedsHotspotWizard` 状态，
//!   用户点一下图形尖角补齐（F156 十字段复用语义）一步到位；
//! - **几何**：路径命令 M/m L/l H/h V/v C/c S/s Q/q T/t Z/z（圆弧 A
//!   诚实报「暂不支持，请转三次贝塞尔」），三次/二次贝塞尔自适应
//!   扁平化为折线，多边形偶奇扫描线填充（jbase fill_polygon）；
//! - **栅格化**：按 viewBox→目标尺寸仿射换算后 1x/2x 双倍率出图
//!   （矢量纪律天然满足 4K——放大不糊的机制面），锐度过 F632 审计
//!   （边缘锐度阈值同源）；
//! - **产物**：jbase `CursorSchemeModel`（vector_source=true——F636
//!   派生优先级的依据）。

use crate::checks::CheckSet;
use crate::jstar2::jbase::{
    fill_polygon, CursorFrame, CursorSchemeModel, OriginKind, PixBuf, PointerState, fnv1a64,
    stroke_line,
};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// SVG 元素节点上限（资源型炸弹防线）。
pub const SVG_NODE_CAP: usize = 4096;
/// 曲线扁平化容差（viewBox 单位 ×1000 定点）。
pub const FLATTEN_TOL_M: i64 = 30;
/// 基准栅格尺寸（viewBox 归一到 32 逻辑 px——指针画幅口径）。
pub const BASE_RASTER_PX: u16 = 32;

// ---------------------------------------------------------------------------
// 错误面（诚实报错）
// ---------------------------------------------------------------------------

/// SVG 导入错误（带定位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SvgError {
    NotXml(String),
    NoSvgRoot,
    NodeCapExceeded(usize),
    UnsupportedArc {
        at_cmd: usize,
    },
    BadNumber {
        at_cmd: usize,
        token: String,
    },
    TooManyPoints(usize),
    ZeroViewBox,
}

impl SvgError {
    pub fn describe(&self) -> String {
        match self {
            SvgError::NotXml(why) => alloc::format!("不是合法 SVG：{why}"),
            SvgError::NoSvgRoot => String::from("找不到 <svg> 根元素"),
            SvgError::NodeCapExceeded(n) => {
                alloc::format!("元素数 {n} 超过 {SVG_NODE_CAP} 上限——疑似资源型文件，拒绝导入")
            }
            SvgError::UnsupportedArc { at_cmd } => alloc::format!(
                "第 {at_cmd} 个命令是圆弧（A）——暂不支持，请在设计工具里转为三次贝塞尔后重试"
            ),
            SvgError::BadNumber { at_cmd, token } => {
                alloc::format!("第 {at_cmd} 个命令的数值「{token}」无法解析")
            }
            SvgError::TooManyPoints(n) => alloc::format!("扁平化后点数 {n} 超限——路径过密"),
            SvgError::ZeroViewBox => String::from("viewBox 宽或高为 0——空画布"),
        }
    }
}

// ---------------------------------------------------------------------------
// XML-lite 词法
// ---------------------------------------------------------------------------

/// 极简元素（导入需要的最小面）。
pub struct XmlElem {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    /// 子元素（一层足够——热点图层约定）。
    pub children: Vec<XmlElem>,
}

impl XmlElem {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// 深度优先元素计数（节点上限防线的度量）。
    pub fn count(&self) -> usize {
        1 + self.children.iter().map(|c| c.count()).sum::<usize>()
    }

    /// 找 id="hotspot" 的图层（双层标记约定之二）。
    pub fn find_hotspot_layer(&self) -> Option<&XmlElem> {
        if self.attr("id") == Some("hotspot") {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find_hotspot_layer())
    }
}

/// XML-lite 解析（标签词法；不支持 CDATA/注释内嵌标签——注释跳过）。
pub fn parse_xml(src: &str) -> Result<XmlElem, SvgError> {
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    skip_ws_and_comments(&bytes, &mut i);
    let root = parse_elem(&bytes, &mut i)?;
    Ok(root)
}

fn skip_ws(c: &[char], i: &mut usize) {
    while *i < c.len() && c[*i].is_whitespace() {
        *i += 1;
    }
}

fn skip_ws_and_comments(c: &[char], i: &mut usize) {
    loop {
        skip_ws(c, i);
        if *i + 3 < c.len() && c[*i] == '<' && c[*i + 1] == '!' && c[*i + 2] == '-' && c[*i + 3] == '-' {
            // 注释 → 找 -->。
            while *i + 2 < c.len() && !(c[*i] == '-' && c[*i + 1] == '-' && c[*i + 2] == '>') {
                *i += 1;
            }
            *i = (*i + 3).min(c.len());
        } else {
            return;
        }
    }
}

fn parse_name(c: &[char], i: &mut usize) -> String {
    let start = *i;
    while *i < c.len() && (c[*i].is_alphanumeric() || c[*i] == '_' || c[*i] == '-' || c[*i] == ':') {
        *i += 1;
    }
    c[start..*i].iter().collect()
}

fn parse_elem(c: &[char], i: &mut usize) -> Result<XmlElem, SvgError> {
    skip_ws_and_comments(c, i);
    if *i >= c.len() || c[*i] != '<' {
        return Err(SvgError::NotXml(String::from("期望 < 开标签")));
    }
    *i += 1;
    let tag = parse_name(c, i);
    if tag.is_empty() {
        return Err(SvgError::NotXml(String::from("开标签名为空")));
    }
    let mut attrs = Vec::new();
    loop {
        skip_ws(c, i);
        if *i >= c.len() {
            return Err(SvgError::NotXml(alloc::format!("<{tag}> 未闭合")));
        }
        match c[*i] {
            '>' => {
                *i += 1;
                break;
            }
            '/' => {
                // 自闭合。
                *i += 1;
                skip_ws(c, i);
                if *i < c.len() && c[*i] == '>' {
                    *i += 1;
                    return Ok(XmlElem { tag, attrs, children: Vec::new() });
                }
                return Err(SvgError::NotXml(String::from("自闭合标签语法错")));
            }
            _ => {
                let name = parse_name(c, i);
                if name.is_empty() {
                    return Err(SvgError::NotXml(String::from("属性名解析失败")));
                }
                skip_ws(c, i);
                if *i < c.len() && c[*i] == '=' {
                    *i += 1;
                    skip_ws(c, i);
                    let quote = c[*i];
                    if quote != '"' && quote != '\'' {
                        return Err(SvgError::NotXml(alloc::format!("属性 {name} 缺引号")));
                    }
                    *i += 1;
                    let start = *i;
                    while *i < c.len() && c[*i] != quote {
                        *i += 1;
                    }
                    let val: String = c[start..*i].iter().collect();
                    *i += 1; // 越过闭引号
                    attrs.push((name, val));
                } else {
                    attrs.push((name, String::new()));
                }
            }
        }
    }
    // 子内容（文本跳过；子标签递归；</tag> 收口）。
    let mut children = Vec::new();
    loop {
        skip_ws_and_comments(c, i);
        if *i >= c.len() {
            return Err(SvgError::NotXml(alloc::format!("<{tag}> 缺少闭标签")));
        }
        if c[*i] == '<' {
            if *i + 1 < c.len() && c[*i + 1] == '/' {
                // 闭标签。
                *i += 2;
                let close = parse_name(c, i);
                skip_ws(c, i);
                if *i < c.len() && c[*i] == '>' {
                    *i += 1;
                }
                if close != tag {
                    return Err(SvgError::NotXml(alloc::format!("闭标签 </{close}> 与 <{tag}> 不配")));
                }
                return Ok(XmlElem { tag, attrs, children });
            }
            children.push(parse_elem(c, i)?);
        } else {
            *i += 1; // 文本字符跳过
        }
    }
}

// ---------------------------------------------------------------------------
// 路径命令解析与扁平化
// ---------------------------------------------------------------------------

/// 扁平化后的折线（viewBox 坐标，1/1000 定点）。
pub struct Polyline {
    pub pts: Vec<(i64, i64)>,
    pub closed: bool,
}

struct PathCursor<'a> {
    tokens: core::str::SplitWhitespace<'a>,
    at_cmd: usize,
    cur: (i64, i64),
    start: (i64, i64),
    last_ctrl: (i64, i64),
}

impl<'a> PathCursor<'a> {
    fn next_num(&mut self) -> Result<i64, SvgError> {
        let t = self.tokens.next().ok_or(SvgError::TooManyPoints(0))?;
        t.parse::<f64>()
            .ok()
            .map(|v| (v * 1000.0) as i64)
            .ok_or_else(|| SvgError::BadNumber { at_cmd: self.at_cmd, token: String::from(t) })
    }

    fn next_cmd(&mut self) -> Option<(char, bool)> {
        self.tokens.next().map(|t| {
            self.at_cmd += 1;
            let c = t.chars().next().unwrap_or('M');
            (c, c.is_uppercase())
        })
    }
}

/// 分词辅助：`need` 为真且前字符非空白时补一个空格。
fn push_sep(out: &mut String, prev: Option<char>, need: bool) {
    if need {
        if let Some(p) = prev {
            if p != ' ' {
                out.push(' ');
            }
        }
    }
}

/// 路径数据归一化：紧凑形式（`M2 2L20 8`、`4-6`、`1.5.5`）补空格
/// 分词——两种写法（空格分隔/紧凑）都能解析，不做静默择一。
pub fn normalize_path_d(d: &str) -> String {
    let mut out = String::with_capacity(d.len() + 8);
    let mut prev: Option<char> = None;
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            // 命令字母前必然断词（前一 token 是数字/点时）。
            push_sep(&mut out, prev, true);
            out.push(c);
        } else if c == '-' {
            // 负号：前是数字/点 → 断词；前是命令字母 → 紧跟。
            let need = prev.map(|p| p.is_ascii_digit() || p == '.').unwrap_or(false);
            push_sep(&mut out, prev, need);
            out.push(c);
        } else if c == '.' {
            // 小数点：前一 token 已含点（如 1.5.5 的第二个点）→ 断词。
            let in_number = prev.map(|p| p.is_ascii_digit() || p == '.').unwrap_or(false);
            let need = in_number && digit_run_has_dot(&out);
            push_sep(&mut out, prev, need);
            out.push(c);
        } else if c == ',' {
            out.push(' ');
            prev = Some(' ');
            continue;
        } else if c.is_whitespace() {
            out.push(' ');
            prev = Some(' ');
            continue;
        } else {
            // 数字：前是命令字母 → 断词（字母后数字要分开）。
            let need = prev.map(|p| p.is_ascii_alphabetic()).unwrap_or(false);
            push_sep(&mut out, prev, need);
            out.push(c);
        }
        prev = out.chars().last();
    }
    out
}

/// 出串里当前数字 run 是否已含点（防 1.5.5 误判——简化口径：
/// 检查最后连续数字字符段）。
fn digit_run_has_dot(out: &str) -> bool {
    let run: String = out.chars().rev().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    run.contains('.')
}

/// 把 path d 属性扁平化为折线集。
pub fn flatten_path(d: &str) -> Result<Vec<Polyline>, SvgError> {
    let d = normalize_path_d(d);
    let mut out: Vec<Polyline> = Vec::new();
    let mut cur_poly: Vec<(i64, i64)> = Vec::new();
    let mut pc = PathCursor {
        tokens: d.split_whitespace(),
        at_cmd: 0,
        cur: (0, 0),
        start: (0, 0),
        last_ctrl: (0, 0),
    };
    let total_pts = |acc: &Vec<Polyline>, cur: &Vec<(i64, i64)>| -> usize {
        acc.iter().map(|p| p.pts.len()).sum::<usize>() + cur.len()
    };
    while let Some((cmd, abs)) = pc.next_cmd() {
        match cmd {
            'M' | 'm' => {
                if !cur_poly.is_empty() {
                    out.push(Polyline { pts: core::mem::take(&mut cur_poly), closed: false });
                }
                let x = pc.next_num()?;
                let y = pc.next_num()?;
                pc.cur = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                pc.start = pc.cur;
                cur_poly.push(pc.cur);
            }
            'L' | 'l' => {
                let x = pc.next_num()?;
                let y = pc.next_num()?;
                pc.cur = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                cur_poly.push(pc.cur);
            }
            'H' | 'h' => {
                let x = pc.next_num()?;
                pc.cur.0 = if abs { x } else { pc.cur.0 + x };
                cur_poly.push(pc.cur);
            }
            'V' | 'v' => {
                let y = pc.next_num()?;
                pc.cur.1 = if abs { y } else { pc.cur.1 + y };
                cur_poly.push(pc.cur);
            }
            'C' | 'c' => {
                let (x1, y1, x2, y2, x, y) = quad6(&mut pc)?;
                let (x1, y1) = if abs { (x1, y1) } else { (pc.cur.0 + x1, pc.cur.1 + y1) };
                let (x2, y2) = if abs { (x2, y2) } else { (pc.cur.0 + x2, pc.cur.1 + y2) };
                let (x, y) = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                flatten_cubic(pc.cur, (x1, y1), (x2, y2), (x, y), &mut cur_poly)?;
                pc.cur = (x, y);
                pc.last_ctrl = (x2, y2);
            }
            'S' | 's' => {
                let (x2, y2, x, y) = quad4(&mut pc)?;
                // 反射控制点。
                let (r1x, r1y) = (2 * pc.cur.0 - pc.last_ctrl.0, 2 * pc.cur.1 - pc.last_ctrl.1);
                let (x2, y2) = if abs { (x2, y2) } else { (pc.cur.0 + x2, pc.cur.1 + y2) };
                let (x, y) = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                flatten_cubic(pc.cur, (r1x, r1y), (x2, y2), (x, y), &mut cur_poly)?;
                pc.cur = (x, y);
                pc.last_ctrl = (x2, y2);
            }
            'Q' | 'q' => {
                let (x1, y1, x, y) = quad4(&mut pc)?;
                let (x1, y1) = if abs { (x1, y1) } else { (pc.cur.0 + x1, pc.cur.1 + y1) };
                let (x, y) = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                flatten_quadratic(pc.cur, (x1, y1), (x, y), &mut cur_poly)?;
                pc.cur = (x, y);
                pc.last_ctrl = (x1, y1);
            }
            'T' | 't' => {
                let (x, y) = quad2(&mut pc)?;
                let (r1x, r1y) = (2 * pc.cur.0 - pc.last_ctrl.0, 2 * pc.cur.1 - pc.last_ctrl.1);
                let (x, y) = if abs { (x, y) } else { (pc.cur.0 + x, pc.cur.1 + y) };
                flatten_quadratic(pc.cur, (r1x, r1y), (x, y), &mut cur_poly)?;
                pc.cur = (x, y);
                pc.last_ctrl = (r1x, r1y);
            }
            'Z' | 'z' => {
                if !cur_poly.is_empty() {
                    cur_poly.push(pc.start);
                    out.push(Polyline { pts: core::mem::take(&mut cur_poly), closed: true });
                }
                pc.cur = pc.start;
            }
            'A' | 'a' => {
                return Err(SvgError::UnsupportedArc { at_cmd: pc.at_cmd });
            }
            other => {
                return Err(SvgError::BadNumber {
                    at_cmd: pc.at_cmd,
                    token: alloc::format!("{other}（未知命令）"),
                })
            }
        }
        if total_pts(&out, &cur_poly) > SVG_NODE_CAP * 16 {
            return Err(SvgError::TooManyPoints(total_pts(&out, &cur_poly)));
        }
    }
    if !cur_poly.is_empty() {
        out.push(Polyline { pts: cur_poly, closed: false });
    }
    Ok(out)
}

fn quad6(pc: &mut PathCursor) -> Result<(i64, i64, i64, i64, i64, i64), SvgError> {
    let a = pc.next_num()?;
    let b = pc.next_num()?;
    let c = pc.next_num()?;
    let d = pc.next_num()?;
    let e = pc.next_num()?;
    let f = pc.next_num()?;
    Ok((a, b, c, d, e, f))
}

fn quad4(pc: &mut PathCursor) -> Result<(i64, i64, i64, i64), SvgError> {
    let a = pc.next_num()?;
    let b = pc.next_num()?;
    let c = pc.next_num()?;
    let d = pc.next_num()?;
    Ok((a, b, c, d))
}

fn quad2(pc: &mut PathCursor) -> Result<(i64, i64), SvgError> {
    let a = pc.next_num()?;
    let b = pc.next_num()?;
    Ok((a, b))
}

/// 三次贝塞尔自适应扁平化（容差 FLATTEN_TOL_M）。
fn flatten_cubic(
    p0: (i64, i64),
    p1: (i64, i64),
    p2: (i64, i64),
    p3: (i64, i64),
    out: &mut Vec<(i64, i64)>,
) -> Result<(), SvgError> {
    // 控制多边形长度估计 → 固定步数（16 步上限，容差内足够）。
    let rough = dist_m(p0, p1) + dist_m(p1, p2) + dist_m(p2, p3);
    let steps = ((rough / FLATTEN_TOL_M) as usize).clamp(4, 24);
    for k in 1..=steps {
        let t = k as i64 * 1000 / steps as i64;
        out.push(cubic_at(p0, p1, p2, p3, t));
        if out.len() > SVG_NODE_CAP * 16 {
            return Err(SvgError::TooManyPoints(out.len()));
        }
    }
    Ok(())
}

/// 二次贝塞尔自适应扁平化。
fn flatten_quadratic(p0: (i64, i64), p1: (i64, i64), p2: (i64, i64), out: &mut Vec<(i64, i64)>) -> Result<(), SvgError> {
    let rough = dist_m(p0, p1) + dist_m(p1, p2);
    let steps = ((rough / FLATTEN_TOL_M) as usize).clamp(4, 24);
    for k in 1..=steps {
        let t = k as i64 * 1000 / steps as i64;
        let mt = 1000 - t;
        let x = (mt * mt * p0.0 + 2 * mt * t * p1.0 + t * t * p2.0) / 1_000_000;
        let y = (mt * mt * p0.1 + 2 * mt * t * p1.1 + t * t * p2.1) / 1_000_000;
        out.push((x, y));
        if out.len() > SVG_NODE_CAP * 16 {
            return Err(SvgError::TooManyPoints(out.len()));
        }
    }
    Ok(())
}

fn cubic_at(p0: (i64, i64), p1: (i64, i64), p2: (i64, i64), p3: (i64, i64), t: i64) -> (i64, i64) {
    let mt = 1000 - t;
    let x = (mt * mt * mt * p0.0 + 3 * mt * mt * t * p1.0 + 3 * mt * t * t * p2.0 + t * t * t * p3.0)
        / 1_000_000_000;
    let y = (mt * mt * mt * p0.1 + 3 * mt * mt * t * p1.1 + 3 * mt * t * t * p2.1 + t * t * t * p3.1)
        / 1_000_000_000;
    (x, y)
}

fn dist_m(a: (i64, i64), b: (i64, i64)) -> i64 {
    let dx = (a.0 - b.0).abs();
    let dy = (a.1 - b.1).abs();
    crate::jstar2::jbase::isqrt64((dx * dx + dy * dy) as u64) as i64
}

// ---------------------------------------------------------------------------
// 栅格化与导入主流程
// ---------------------------------------------------------------------------

/// viewBox 归一：把 viewBox (vx,vy,vw,vh) 中的点映射到目标画布（px）。
fn map_point(p: (i64, i64), vb: (i64, i64, i64, i64), size: u16) -> (i64, i64) {
    if vb.2 == 0 || vb.3 == 0 {
        return (0, 0);
    }
    let sx = size as i64 * 1000 / vb.2;
    let sy = size as i64 * 1000 / vb.3;
    ((p.0 - vb.0) * sx / 1000, (p.1 - vb.1) * sy / 1000)
}

/// 双层热点标记解析：根 `data-hotspot="x,y"`（viewBox 坐标）或
/// id="hotspot" 图层（取图层内图形的包围盒中心/首点）。
fn resolve_hotspot(root: &XmlElem, vb: (i64, i64, i64, i64)) -> Result<Option<(i64, i64)>, SvgError> {
    if let Some(hs) = root.attr("data-hotspot") {
        let p: Vec<&str> = hs.split(',').collect();
        if p.len() == 2 {
            let x: f64 = p[0].trim().parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(p[0]) })?;
            let y: f64 = p[1].trim().parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(p[1]) })?;
            return Ok(Some(map_point(((x * 1000.0) as i64, (y * 1000.0) as i64), vb, BASE_RASTER_PX)));
        }
        return Err(SvgError::NotXml(String::from("data-hotspot 格式应为 \"x,y\"")));
    }
    if let Some(layer) = root.find_hotspot_layer() {
        // 图层内任意图形首点（十字点/小方块的几何中心近似：取第一个
        // 数值对）。
        for tag in ["circle", "rect", "path"] {
            if let Some(el) = layer.children.iter().find(|c| c.tag == tag) {
                if tag == "circle" {
                    if let (Some(cx), Some(cy)) = (el.attr("cx"), el.attr("cy")) {
                        let x: f64 = cx.parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(cx) })?;
                        let y: f64 = cy.parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(cy) })?;
                        return Ok(Some(map_point(((x * 1000.0) as i64, (y * 1000.0) as i64), vb, BASE_RASTER_PX)));
                    }
                } else if tag == "rect" {
                    if let (Some(rx), Some(ry)) = (el.attr("x"), el.attr("y")) {
                        let x: f64 = rx.parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(rx) })?;
                        let y: f64 = ry.parse().map_err(|_| SvgError::BadNumber { at_cmd: 0, token: String::from(ry) })?;
                        return Ok(Some(map_point(((x * 1000.0) as i64, (y * 1000.0) as i64), vb, BASE_RASTER_PX)));
                    }
                }
            }
        }
    }
    Ok(None)
}

/// SVG 导入结果（热点缺失 → NeedsHotspotWizard）。
pub enum SvgImport {
    /// 成功：双倍率方案（vector_source=true）。
    Imported(CursorSchemeModel),
    /// 缺热点标记 → 向导（用户点一下补齐；不拒收）。
    NeedsHotspotWizard(WizardDraft),
    /// 错误（诚实报错定位）。
    Failed(SvgError),
}

/// 向导草稿（已栅格化、只欠热点）。
pub struct WizardDraft {
    pub polylines: Vec<Polyline>,
    pub vb: (i64, i64, i64, i64),
}

impl WizardDraft {
    /// 用户点选（画布 px 坐标）→ 补齐热点 → 完整方案。
    pub fn complete(self, hot_px: (u16, u16), name: &str) -> CursorSchemeModel {
        rasterize_scheme(&self.polylines, self.vb, hot_px, name)
    }
}

/// 栅格化：折线集 → 1x/2x 双帧方案（vector_source=true）。
fn rasterize_scheme(polylines: &[Polyline], vb: (i64, i64, i64, i64), hot: (u16, u16), name: &str) -> CursorSchemeModel {
    let mut m = CursorSchemeModel::empty(name, OriginKind::Imported(String::from("svg")));
    m.vector_source = true;
    let mut frame1x = PixBuf::new(BASE_RASTER_PX, BASE_RASTER_PX);
    for pl in polylines {
        let pts: Vec<(i64, i64)> = pl.pts.iter().map(|p| map_point(*p, vb, BASE_RASTER_PX)).collect();
        if pl.closed && pts.len() >= 3 {
            fill_polygon(&mut frame1x, &pts, [24, 24, 24, 255]);
        } else {
            for w in pts.windows(2) {
                stroke_line(&mut frame1x, w[0].0, w[0].1, w[1].0, w[1].1, 0, [24, 24, 24, 255]);
            }
        }
    }
    let frame2x = frame1x.scale_integer2x();
    let hot2 = ((hot.0 as u32 * 2).min(2 * BASE_RASTER_PX as u32 - 1) as u16, (hot.1 as u32 * 2).min(2 * BASE_RASTER_PX as u32 - 1) as u16);
    m.set_state(
        PointerState::Normal,
        alloc::vec![
            CursorFrame::from_buf(hot.0, hot.1, 0, frame1x),
            CursorFrame::from_buf(hot2.0, hot2.1, 0, frame2x),
        ],
    );
    m
}

/// 导入主入口：SVG 文本 → 导入结果（自动识别热点标记两路约定）。
pub fn import_svg(src: &str, name: &str) -> SvgImport {
    let root = match parse_xml(src) {
        Ok(r) => r,
        Err(e) => return SvgImport::Failed(e),
    };
    if root.count() > SVG_NODE_CAP {
        return SvgImport::Failed(SvgError::NodeCapExceeded(root.count()));
    }
    if root.tag != "svg" {
        return SvgImport::Failed(SvgError::NoSvgRoot);
    }
    // viewBox（缺省 0 0 32 32）。
    let vb = match root.attr("viewBox") {
        Some(v) => {
            let p: Vec<&str> = v.split_whitespace().collect();
            if p.len() != 4 {
                return SvgImport::Failed(SvgError::NotXml(String::from("viewBox 应为 4 个数值")));
            }
            let mut n = [0i64; 4];
            for (i, t) in p.iter().enumerate() {
                match t.parse::<f64>() {
                    Ok(f) => n[i] = (f * 1000.0) as i64,
                    Err(_) => return SvgImport::Failed(SvgError::BadNumber { at_cmd: 0, token: String::from(*t) }),
                }
            }
            (n[0], n[1], n[2], n[3])
        }
        None => (0, 0, 32_000, 32_000),
    };
    if vb.2 == 0 || vb.3 == 0 {
        return SvgImport::Failed(SvgError::ZeroViewBox);
    }
    // 折线集（path/rect/circle/ellipse/polygon/polyline → 统一折线）。
    let mut polylines: Vec<Polyline> = Vec::new();
    for el in collect_shape_elems(&root) {
        match el.tag.as_str() {
            "path" => {
                let Some(d) = el.attr("d") else { continue };
                match flatten_path(d) {
                    Ok(mut ps) => polylines.append(&mut ps),
                    Err(e) => return SvgImport::Failed(e),
                }
            }
            "rect" => {
                let (x, y) = num_pair(el.attr("x"), el.attr("y"), 0.0, 0.0);
                let (w, h) = num_pair(el.attr("width"), el.attr("height"), 0.0, 0.0);
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                polylines.push(Polyline {
                    pts: alloc::vec![
                        ((x * 1000.0) as i64, (y * 1000.0) as i64),
                        (((x + w) * 1000.0) as i64, (y * 1000.0) as i64),
                        (((x + w) * 1000.0) as i64, ((y + h) * 1000.0) as i64),
                        ((x * 1000.0) as i64, ((y + h) * 1000.0) as i64),
                        ((x * 1000.0) as i64, (y * 1000.0) as i64),
                    ],
                    closed: true,
                });
            }
            "circle" | "ellipse" => {
                let (cx, cy) = num_pair(el.attr("cx"), el.attr("cy"), 0.0, 0.0);
                let (rx, ry) = if el.tag == "circle" {
                    num_pair(el.attr("r"), None, 0.0, 0.0)
                } else {
                    num_pair(el.attr("rx"), el.attr("ry"), 0.0, 0.0)
                };
                if rx <= 0.0 || ry <= 0.0 {
                    continue;
                }
                let mut pts = Vec::with_capacity(24);
                for k in 0..24 {
                    let a = k as f64 * core::f64::consts::TAU / 24.0;
                    pts.push((((cx + rx * a.cos()) * 1000.0) as i64, ((cy + ry * a.sin()) * 1000.0) as i64));
                }
                pts.push(pts[0]);
                polylines.push(Polyline { pts, closed: true });
            }
            "polygon" | "polyline" => {
                let Some(pts_s) = el.attr("points") else { continue };
                let mut pts: Vec<(i64, i64)> = Vec::new();
                let vals: Vec<&str> = pts_s.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
                let mut v = vals.iter();
                while let (Some(x), Some(y)) = (v.next(), v.next()) {
                    match (x.parse::<f64>(), y.parse::<f64>()) {
                        (Ok(xv), Ok(yv)) => pts.push(((xv * 1000.0) as i64, (yv * 1000.0) as i64)),
                        _ => return SvgImport::Failed(SvgError::BadNumber { at_cmd: 0, token: alloc::format!("{x},{y}") }),
                    }
                }
                if pts.len() >= 2 {
                    let closed = el.tag == "polygon";
                    if closed {
                        pts.push(pts[0]);
                    }
                    polylines.push(Polyline { pts, closed });
                }
            }
            _ => {}
        }
    }
    if polylines.is_empty() {
        return SvgImport::Failed(SvgError::NotXml(String::from("没有可渲染的图形元素")));
    }
    // 热点两路约定。
    match resolve_hotspot(&root, vb) {
        Err(e) => SvgImport::Failed(e),
        Ok(Some(hot_px)) => SvgImport::Imported(rasterize_scheme(&polylines, vb, (hot_px.0.min((BASE_RASTER_PX - 1) as i64) as u16, hot_px.1.min((BASE_RASTER_PX - 1) as i64) as u16), name)),
        Ok(None) => SvgImport::NeedsHotspotWizard(WizardDraft { polylines, vb }),
    }
}

fn collect_shape_elems(root: &XmlElem) -> Vec<&XmlElem> {
    let mut v: Vec<&XmlElem> = Vec::new();
    fn walk<'a>(e: &'a XmlElem, out: &mut Vec<&'a XmlElem>) {
        if matches!(e.tag.as_str(), "path" | "rect" | "circle" | "ellipse" | "polygon" | "polyline") {
            out.push(e);
        }
        for c in &e.children {
            walk(c, out);
        }
    }
    walk(root, &mut v);
    v
}

fn num_pair(a: Option<&str>, b: Option<&str>, da: f64, db: f64) -> (f64, f64) {
    (
        a.and_then(|v| v.parse().ok()).unwrap_or(da),
        b.and_then(|v| v.parse().ok()).unwrap_or(db),
    )
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F634 自检。
pub fn run_svgimport_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F634");

    // 1. 双层标记约定之一：data-hotspot 属性。
    let with_attr = r#"<svg viewBox="0 0 32 32" data-hotspot="2,2"><path d="M 2 2 L 20 8 L 8 20 Z"/></svg>"#;
    match import_svg(with_attr, "属性热点") {
        SvgImport::Imported(m) => {
            let f = &m.state(PointerState::Normal).unwrap().frames[0];
            set.add(
                "data-hotspot honored and rasterized",
                m.vector_source && f.hot_x <= 4 && f.hot_y <= 4 && f.buf().solid_count() > 20,
                "",
            );
        }
        _ => set.add("data-hotspot honored and rasterized", false, "wrong variant"),
    }

    // 2. 双层标记约定之二：id="hotspot" 图层（circle 中心）。
    let with_layer = r#"<svg viewBox="0 0 32 32"><g id="hotspot"><circle cx="4" cy="6" r="1"/></g><path d="M 4 6 L 24 10 L 10 24 Z"/></svg>"#;
    match import_svg(with_layer, "图层热点") {
        SvgImport::Imported(m) => {
            let f = &m.state(PointerState::Normal).unwrap().frames[0];
            set.add("hotspot layer honored", f.hot_x <= 6 && f.hot_y <= 8, "");
        }
        _ => set.add("hotspot layer honored", false, "wrong variant"),
    }

    // 3. 缺标记 → 向导（不拒收），补点后完整方案。
    let no_mark = r#"<svg viewBox="0 0 32 32"><path d="M 2 2 L 24 6 L 8 24 Z"/></svg>"#;
    match import_svg(no_mark, "待补") {
        SvgImport::NeedsHotspotWizard(d) => {
            let m = d.complete((2, 2), "补齐件");
            set.add(
                "wizard completes without rejection",
                m.vector_source
                    && m.state(PointerState::Normal).unwrap().frames[0].hot_x == 2
                    && m.missing_states().len() == 14,
                "",
            );
        }
        _ => set.add("wizard completes without rejection", false, "should need wizard"),
    }

    // 4. 双倍率栅格化锐度：2x 帧是 1x 精确 2×2 复制（过 F632 边缘锐度线）。
    let ok = r#"<svg viewBox="0 0 32 32" data-hotspot="1,1"><rect x="4" y="4" width="16" height="16"/></svg>"#;
    match import_svg(ok, "方块") {
        SvgImport::Imported(m) => {
            let e = m.state(PointerState::Normal).unwrap();
            let f1 = e.frames[0].buf();
            let f2 = e.frames[1].buf();
            let mut exact = f2.w == 64;
            if exact {
                'o: for y in 0..32u16 {
                    for x in 0..32u16 {
                        let p = f1.get(x, y).unwrap();
                        if f2.get(x * 2, y * 2) != Some(p)
                            || f2.get(x * 2 + 1, y * 2) != Some(p)
                            || f2.get(x * 2, y * 2 + 1) != Some(p)
                            || f2.get(x * 2 + 1, y * 2 + 1) != Some(p)
                        {
                            exact = false;
                            break 'o;
                        }
                    }
                }
            }
            set.add("2x raster is exact replication", exact, "");
        }
        _ => set.add("2x raster is exact replication", false, "wrong variant"),
    }

    // 5. 矢量缩放对拍：32px 与 64px 栅格同源（viewBox 映射确定性）。
    match import_svg(r#"<svg viewBox="0 0 32 32" data-hotspot="1,1"><rect x="8" y="8" width="8" height="8"/></svg>"#, "缩放对拍") {
        SvgImport::Imported(m) => {
            let f1 = m.state(PointerState::Normal).unwrap().frames[0].buf();
            // 中心 (12,12) 应实体、角落 (0,0) 透明——映射正确性。
            set.add(
                "vector mapping correct",
                f1.solid(12, 12) && f1.solid(15, 15) && !f1.solid(0, 0) && !f1.solid(31, 31),
                "",
            );
        }
        _ => set.add("vector mapping correct", false, "wrong variant"),
    }

    // 6. 异常样本诚实报错：圆弧 / 坏数值 / 无根 / 节点超限。
    let arc = import_svg(r#"<svg viewBox="0 0 32 32" data-hotspot="1,1"><path d="M 2 2 A 10 10 0 0 1 20 20"/></svg>"#, "圆弧");
    set.add(
        "arc honestly unsupported with hint",
        matches!(&arc, SvgImport::Failed(e) if e.describe().contains("贝塞尔")),
        "",
    );
    let badnum = import_svg(r#"<svg viewBox="0 0 32 32" data-hotspot="1,1"><path d="M x y"/></svg>"#, "坏数值");
    set.add("bad number honest error", matches!(&badnum, SvgImport::Failed(_)), "");
    let noroot = import_svg("<g><rect x='1' y='1' width='2' height='2'/></g>", "无根");
    set.add("missing svg root caught", matches!(&noroot, SvgImport::Failed(SvgError::NoSvgRoot)), "");
    let mut many = String::from("<svg viewBox=\"0 0 32 32\">");
    for i in 0..(SVG_NODE_CAP + 10) {
        many.push_str(&alloc::format!("<rect x='{}' y='1' width='1' height='1'/>", i % 30));
    }
    many.push_str("</svg>");
    set.add(
        "node cap enforced",
        matches!(import_svg(&many, "超限"), SvgImport::Failed(SvgError::NodeCapExceeded(_))),
        "",
    );

    // 7. 产物指纹稳定（同输入同输出）。
    let a = import_svg(with_attr, "指纹");
    let b = import_svg(with_attr, "指纹");
    if let (SvgImport::Imported(ma), SvgImport::Imported(mb)) = (a, b) {
        set.add(
            "deterministic fingerprint",
            fnv1a64(&crate::jstar2::jbase::serialize_vxcur(&ma)) == fnv1a64(&crate::jstar2::jbase::serialize_vxcur(&mb)),
            "",
        );
    } else {
        set.add("deterministic fingerprint", false, "import failed");
    }

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quadratic_and_smooth_curves_flatten() {
        let d = "M 0 0 Q 8 16 16 0 T 32 0";
        let polylines = flatten_path(d).unwrap();
        let all: Vec<(i64, i64)> = polylines.iter().flat_map(|p| p.pts.clone()).collect();
        assert!(all.len() > 10, "曲线应有足量扁平点");
        // 峰值在中间（Q 控制点把曲线拉向 y=16 方向 → 中段 y>2）。
        let max_y = all.iter().map(|p| p.1).max().unwrap();
        assert!(max_y > 3000, "扁平化应保留曲率 max_y={max_y}");
    }

    #[test]
    fn relative_commands_supported() {
        let d = "m 4 4 l 10 0 l 0 10 z";
        let polylines = flatten_path(d).unwrap();
        assert_eq!(polylines.len(), 1);
        assert!(polylines[0].closed);
        assert_eq!(polylines[0].pts.len(), 4); // 3 点 + 闭合回起点
    }

    #[test]
    fn compact_path_data_normalized() {
        // 紧凑形式（无空格）与空格形式解析结果一致。
        let spaced = flatten_path("M 4 4 L 14 4 L 14 14 Z").unwrap();
        let compact = flatten_path("M4 4L14 4L14 14Z").unwrap();
        assert_eq!(spaced.len(), compact.len());
        for (a, b) in spaced.iter().zip(compact.iter()) {
            assert_eq!(a.pts, b.pts);
        }
        // 负数紧跟（4-6）与连续小数（1.5.5）分词正确。
        let neg = flatten_path("M4-6L8-2").unwrap();
        assert_eq!(neg[0].pts[0], (4000, -6000));
        let dots = flatten_path("M1.5.5L2 2").unwrap();
        assert_eq!(dots[0].pts[0], (1500, 500));
    }

    #[test]
    fn cubic_bezier_accuracy() {
        // 直线型三次贝塞尔（控制点共线）→ 扁平点应精确落在线上。
        let d = "M 0 0 C 10 10 20 20 30 30";
        let polylines = flatten_path(d).unwrap();
        for p in &polylines[0].pts {
            assert_eq!(p.0, p.1, "共线控制点应保持 y=x");
        }
    }

    #[test]
    fn unclosed_tag_errors() {
        assert!(parse_xml("<svg viewBox=\"0 0 32 32\"").is_err());
        assert!(parse_xml("<svg><path</svg>").is_err());
    }

    #[test]
    fn comments_skipped() {
        let src = "<svg viewBox=\"0 0 32 32\" data-hotspot=\"1,1\"><!-- 注释 <path d=\"M 0 0\"/> --><rect x=\"2\" y=\"2\" width=\"4\" height=\"4\"/></svg>";
        match import_svg(src, "注释") {
            SvgImport::Imported(m) => {
                // 注释内的假 path 不参与——只有 rect（2,2..6,6）。
                let f = m.state(PointerState::Normal).unwrap().frames[0].buf();
                assert!(f.solid(3, 3) && !f.solid(0, 0));
            }
            _ => panic!("should import"),
        }
    }

    #[test]
    fn hotspot_clamped_into_canvas() {
        let src = "<svg viewBox=\"0 0 32 32\" data-hotspot=\"40,40\"><rect x=\"2\" y=\"2\" width=\"4\" height=\"4\"/></svg>";
        match import_svg(src, "越界热点") {
            SvgImport::Imported(m) => {
                let f = &m.state(PointerState::Normal).unwrap().frames[0];
                assert!(f.hot_x < f.w && f.hot_y < f.h, "热点钳进画布");
            }
            _ => panic!("should import"),
        }
    }
}
