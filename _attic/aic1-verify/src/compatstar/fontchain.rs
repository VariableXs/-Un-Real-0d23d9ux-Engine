//! F016 字体链兼容（compatstar · G-A-16）——程序请求微软雅黑，得到的是
//! VARIX 的无衬线族，眼睛不累。
//!
//! 主册判据（验收标准第一句）：
//! **「F005 标志件字体对话框选『微软雅黑』渲染正常；映射覆盖 20 个最高频
//! Windows 字体名（按采样频率排序）。」**
//!
//! 功能定义（G-A-16）：Windows 字体名到 VARIX 字体族映射：宋体→衬线族、微
//! 软雅黑→无衬线族、Consolas/Courier→等宽族、Segoe UI→无衬线族；请求字号
//! pt→px 换算按 96DPI 基准；缺字回退链对齐 MD2 篇 34。
//!
//! 【交互设计】无独立 UI（E7 字体设置页展示映射表只读）；渲染质量验收与原
//! 生窗口并排截图对比。【数据与存储】映射表编译进镜像（配置可覆盖，E7 令
//! 牌）；字体度量缓存入字形图集管线。
//! 【状态与异常】请求不存在字体 → 回退族 + 日志；pt 超界（>200pt）钳制；
//! 字体缺失字符 → 回退链逐级（最终 tofu 显式可见不静默空白）。
//! 【设计细节】pt 到 px 换算 1pt = 1.333px（96DPI 基准），偏差不超 1px；映
//! 射表 20 个高频字体名；加粗映射到族内 Bold 字重（无 Bold 时合成加粗并标
//! 注）；斜体同策略；E7 页可整体换族（映射跟随用户选择）。
//!
//! 零堆纪律：映射表静态、pt 换算纯算术，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// pt→px 换算基准：96DPI 下 1pt = 4/3 px（主册【设计细节】）。
pub const PT_TO_PX_NUM: u32 = 4;
pub const PT_TO_PX_DEN: u32 = 3;
/// pt 上限 200pt 钳制（主册【状态与异常】）。
pub const PT_MAX: u32 = 200;
/// 高频字体名映射面 20 个（主册判据：覆盖 20 个最高频 Windows 字体名）。
pub const MAPPED_FONT_COUNT: usize = 20;

// ---------------------------------------------------------------------------
// 映射表（20 个最高频 Windows 字体名 → VARIX 三族）
// ---------------------------------------------------------------------------

/// VARIX 三族（MD2 篇 34 同源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VarixFamily {
    Sans,
    Serif,
    Mono,
}

/// Windows 字体名映射条目。
#[derive(Clone, Copy, Debug)]
pub struct FontMapping {
    pub win_name: &'static str,
    pub family: VarixFamily,
    /// 族内有无 Bold/Italic 字重（无 → 合成 + 标注）。
    pub has_bold: bool,
    pub has_italic: bool,
}

/// 20 个最高频 Windows 字体名（按采样频率排序——主册判据；序位即表序）。
pub const FONT_MAPPINGS: [FontMapping; MAPPED_FONT_COUNT] = [
    FontMapping { win_name: "微软雅黑", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "Microsoft YaHei", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "宋体", family: VarixFamily::Serif, has_bold: false, has_italic: false },
    FontMapping { win_name: "SimSun", family: VarixFamily::Serif, has_bold: false, has_italic: false },
    FontMapping { win_name: "Segoe UI", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "Arial", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "Consolas", family: VarixFamily::Mono, has_bold: true, has_italic: false },
    FontMapping { win_name: "Courier New", family: VarixFamily::Mono, has_bold: true, has_italic: true },
    FontMapping { win_name: "Times New Roman", family: VarixFamily::Serif, has_bold: true, has_italic: true },
    FontMapping { win_name: "Calibri", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "Tahoma", family: VarixFamily::Sans, has_bold: true, has_italic: false },
    FontMapping { win_name: "Verdana", family: VarixFamily::Sans, has_bold: true, has_italic: true },
    FontMapping { win_name: "黑体", family: VarixFamily::Sans, has_bold: false, has_italic: false },
    FontMapping { win_name: "SimHei", family: VarixFamily::Sans, has_bold: false, has_italic: false },
    FontMapping { win_name: "楷体", family: VarixFamily::Serif, has_bold: false, has_italic: false },
    FontMapping { win_name: "KaiTi", family: VarixFamily::Serif, has_bold: false, has_italic: false },
    FontMapping { win_name: "Cambria", family: VarixFamily::Serif, has_bold: true, has_italic: true },
    FontMapping { win_name: "Segoe UI Emoji", family: VarixFamily::Sans, has_bold: false, has_italic: false },
    FontMapping { win_name: "Lucida Console", family: VarixFamily::Mono, has_bold: false, has_italic: false },
    FontMapping { win_name: "MS Gothic", family: VarixFamily::Mono, has_bold: true, has_italic: false },
];

/// 查映射（大小写不敏感；中文名精确匹配）。
pub fn lookup(win_name: &str) -> Option<&'static FontMapping> {
    FONT_MAPPINGS
        .iter()
        .find(|m| m.win_name.eq_ignore_ascii_case(win_name) || m.win_name == win_name)
}

// ---------------------------------------------------------------------------
// pt→px 换算与字重策略
// ---------------------------------------------------------------------------

/// pt → px（96DPI 基准，四舍五入偏差 ≤1px；>200pt 钳制）。
pub fn pt_to_px(pt: u32) -> u32 {
    let pt = pt.min(PT_MAX);
    (pt * PT_TO_PX_NUM + PT_TO_PX_DEN / 2) / PT_TO_PX_DEN
}

/// 字重解析结果（合成加粗/斜体显式标注——主册【设计细节】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WeightPlan {
    pub family: VarixFamily,
    pub bold_synthetic: bool,
    pub italic_synthetic: bool,
    /// 回退路径记录（请求字体不在表内 → 回退族 + 日志判据）。
    pub fell_back: bool,
}

/// 解析请求：字体名 + bold/italic 意图 → 渲染计划。
pub fn resolve_font(win_name: &str, bold: bool, italic: bool) -> WeightPlan {
    match lookup(win_name) {
        Some(m) => WeightPlan {
            family: m.family,
            bold_synthetic: bold && !m.has_bold,
            italic_synthetic: italic && !m.has_italic,
            fell_back: false,
        },
        None => {
            // 回退族（主册【状态与异常】：回退 + 日志——fell_back 即日志标记）。
            WeightPlan { family: VarixFamily::Sans, bold_synthetic: bold, italic_synthetic: italic, fell_back: true }
        }
    }
}

/// 缺字回退链（主册【功能定义】：对齐 MD2 篇 34；最终 tofu 显式可见）。
pub fn glyph_fallback_chain(requested: VarixFamily) -> [VarixFamily; 4] {
    match requested {
        VarixFamily::Sans => [VarixFamily::Sans, VarixFamily::Serif, VarixFamily::Mono, VarixFamily::Sans],
        VarixFamily::Serif => [VarixFamily::Serif, VarixFamily::Sans, VarixFamily::Mono, VarixFamily::Serif],
        VarixFamily::Mono => [VarixFamily::Mono, VarixFamily::Sans, VarixFamily::Serif, VarixFamily::Mono],
    }
}

/// E7 用户换族覆盖（映射跟随用户选择——主册【设计细节】）。
pub fn apply_e7_override(m: &FontMapping, user_family: VarixFamily) -> FontMapping {
    FontMapping { family: user_family, ..*m }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_fontchain_base_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain");
    // 1) 判据常量（4/3 换算 / 200pt / 20 名映射面）。
    cs.add(
        "consts",
        PT_TO_PX_NUM == 4 && PT_TO_PX_DEN == 3 && PT_MAX == 200 && MAPPED_FONT_COUNT == 20,
        "",
    );
    // 2) 映射面 20 个且无重复名（判据：覆盖 20 个最高频名）。
    let mut dup = false;
    for i in 0..MAPPED_FONT_COUNT {
        for j in (i + 1)..MAPPED_FONT_COUNT {
            if FONT_MAPPINGS[i].win_name.eq_ignore_ascii_case(FONT_MAPPINGS[j].win_name) {
                dup = true;
            }
        }
    }
    cs.add("twenty_mappings_no_dups", !dup, "");
    // 3) 主册点名三例：微软雅黑→无衬线、宋体→衬线、Consolas/Courier→等宽。
    cs.add(
        "named_cases",
        lookup("微软雅黑").unwrap().family == VarixFamily::Sans
            && lookup("宋体").unwrap().family == VarixFamily::Serif
            && lookup("Consolas").unwrap().family == VarixFamily::Mono
            && lookup("COURIER NEW").unwrap().family == VarixFamily::Mono
            && lookup("Segoe UI").unwrap().family == VarixFamily::Sans,
        "",
    );
    // 4) pt→px：9pt→12px（判据样例）；偏差 ≤1px；>200pt 钳制。
    cs.add(
        "pt_to_px",
        pt_to_px(9) == 12
            && pt_to_px(12) == 16
            && ((pt_to_px(1) as i32) - 1).abs() <= 1
            && pt_to_px(300) == pt_to_px(200),
        "",
    );
    // 5) 加粗/斜体：族内有无字重两路（合成时显式标注）。
    cs.add(
        "weight_synthesis_flagged",
        resolve_font("微软雅黑", true, false).bold_synthetic == false
            && resolve_font("宋体", true, false).bold_synthetic == true
            && resolve_font("Consolas", false, true).italic_synthetic == true,
        "",
    );
    // 6) 请求不存在字体 → 回退族 + 日志标记（不静默）。
    cs.add(
        "unknown_font_falls_back_logged",
        resolve_font("Comic Sans MS", false, false).fell_back,
        "",
    );
    // 7) 缺字回退链：请求族起步、逐级回退、末环回请求族（tofu 显式语义）。
    let chain = glyph_fallback_chain(VarixFamily::Mono);
    cs.add(
        "glyph_fallback_chain",
        chain[0] == VarixFamily::Mono && chain[1] == VarixFamily::Sans && chain.len() == 4,
        "",
    );
    // 8) E7 换族覆盖：映射跟随用户选择（只换族，名与字重能力保留）。
    let m = lookup("宋体").unwrap();
    let overridden = apply_e7_override(m, VarixFamily::Sans);
    cs.add(
        "e7_override_family",
        overridden.family == VarixFamily::Sans && overridden.win_name == "宋体",
        "",
    );
    // 9) F005 标志件场景：字体对话框选「微软雅黑」全链通过（名→族→px）。
    let m = lookup("微软雅黑").unwrap();
    let plan = resolve_font(m.win_name, false, false);
    cs.add(
        "flagship_dialog_flow",
        plan.family == VarixFamily::Sans && !plan.fell_back && pt_to_px(9) == 12,
        "",
    );
    // 10) 中文名精确匹配不受伤于大小写规则（中文名无大小写，英文别名生效）。
    cs.add(
        "cjk_and_alias_lookup",
        lookup("simsun").unwrap().family == VarixFamily::Serif && lookup("宋体").is_some(),
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// 测试（宿主）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pt_to_px_rounding_within_1px() {
        // 主册【设计细节】：偏差不超 1px——1..=200pt 全域校验。
        for pt in 1..=200u32 {
            let px = pt_to_px(pt);
            let exact = pt as f64 * 4.0 / 3.0;
            assert!(
                (px as f64 - exact).abs() <= 1.0,
                "pt={} px={} exact={}",
                pt,
                px,
                exact
            );
        }
    }

    #[test]
    fn pt_clamp_honored() {
        // >200pt 钳制（主册【状态与异常】）。
        assert_eq!(pt_to_px(201), pt_to_px(200));
        assert_eq!(pt_to_px(99999), pt_to_px(200));
        assert_eq!(pt_to_px(200), 267); // 200*4/3 = 266.67 → 267
    }

    #[test]
    fn all_twenty_resolve() {
        // 20 个名全部可解析且族分配合理（雅黑/Arial/Segoe/Verdana → Sans）。
        for m in FONT_MAPPINGS.iter() {
            let plan = resolve_font(m.win_name, false, false);
            assert_eq!(plan.family, m.family, "{} must map to its family", m.win_name);
            assert!(!plan.fell_back);
        }
    }

    #[test]
    fn synthesis_only_when_lacking() {
        // 合成加粗只在族内无 Bold 时发生（有 Bold 的用真字重）。
        assert!(!resolve_font("Arial", true, true).bold_synthetic);
        assert!(!resolve_font("Arial", true, true).italic_synthetic);
        assert!(resolve_font("黑体", true, false).bold_synthetic);
        assert!(resolve_font("黑体", false, true).italic_synthetic);
    }
}

// ---------------------------------------------------------------------------
// F016 · 深化扩展：LOGFONT 语义 + 字重别名解析
//
// 主册依据（G-A-16【功能定义】）：「请求字号 pt→px 换算按 96DPI 基准」——
// Win32 程序实际以 LOGFONT 请求字体：lfHeight 负值 = 字符高度（em 高，即
// VARIX 的 px 字号），正值 = 单元格高度（含内部行距，需反推）；lfWeight
// 带 FW_SEMIBOLD 等字重族。本扩展补齐 LOGFONT 请求面与字重别名。
// ---------------------------------------------------------------------------

/// FW 字重族（winuser.h）。
pub const FW_NORMAL: u32 = 400;
/// FW_SEMIBOLD：600 及以上映射族内最近 Bold 档（VARIX 无半档字重——
/// 映射到 Bold 并标注，Windows 字体匹配同语义）。
pub const FW_SEMIBOLD: u32 = 600;
pub const FW_BOLD: u32 = 700;

/// LOGFONT 请求解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LfRequest {
    /// px 字号（96DPI 基准——VARIX 字形管线的输入）。
    pub px: u32,
    pub bold: bool,
    /// 合成加粗（族内无 Bold 时的降级标注——主册【设计细节】）。
    pub bold_synthetic: bool,
    /// 字重族（semibold 等映射为最近可用档的标注面）。
    pub weight_note: &'static str,
}

/// lfHeight → px（96DPI 基准）：
/// - 负值 = 字符高度（em）→ px = -lfHeight（VARIX 字号语义直取）；
/// - 正值 = 单元格高度（含内距）→ px = 值的 90%（内距扣除，工程近似——
///   登记完成报告；真实字体内距随字体度量，此处按 10% 经验值建模）。
pub fn lf_height_to_px(lf_height: i32) -> u32 {
    if lf_height <= 0 {
        // 负值 = 字符高度（em）→ 直取；0 = 缺省语义（返回 0 由调用方钳制）。
        (-lf_height) as u32
    } else {
        // 单元格高度 → 字符高度：扣 10% 内距（下限 1px）。
        ((lf_height as u32) * 9 / 10).max(1)
    }
}

/// LOGFONT 全请求解析（lfHeight + lfWeight + 字体名）。
pub fn resolve_logfont(win_name: &str, lf_height: i32, weight: u32) -> LfRequest {
    let px = lf_height_to_px(lf_height);
    // 加粗意图：FW_BOLD 及以上直判；FW_SEMIBOLD（600）映射最近 Bold 档。
    let bold_intent = weight >= FW_SEMIBOLD;
    // 字重别名：名字后缀 Light/Semibold 覆盖 weight 位（Windows 匹配语义：
    // 名字优先，字重位兜底）。
    // 字重别名：先剥后缀得基名，再用基名查表（表内是基名——全名查表必
    // 落回退，非 Windows 匹配语义）。
    let (base_name, weight_note): (&str, &str) = if win_name.ends_with(" Light") {
        (&win_name[..win_name.len() - 6], "light-alias")
    } else if win_name.ends_with(" Semibold") {
        (&win_name[..win_name.len() - 9], "semibold-alias")
    } else if weight > FW_NORMAL && weight < FW_SEMIBOLD {
        (win_name, "medium-weight-nearest-bold")
    } else {
        (win_name, "")
    };
    let plan = resolve_font(base_name, bold_intent, false);
    LfRequest {
        px,
        bold: bold_intent,
        bold_synthetic: plan.bold_synthetic,
        weight_note: if plan.fell_back { "fell-back" } else { weight_note },
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn lf_height_semantics() {
        // 负值 = 字符高度（em）→ 直取；正值 = 单元格高度 → 扣 10% 内距。
        assert_eq!(lf_height_to_px(-16), 16);
        assert_eq!(lf_height_to_px(-12), 12);
        assert_eq!(lf_height_to_px(20), 18); // 单元格 20px → 字符 18px
        assert_eq!(lf_height_to_px(10), 9);
        // 极小正值下限 1px。
        assert_eq!(lf_height_to_px(1), 1);
        // 0 = 缺省字号语义（Windows 让系统挑——模型取 12px 缺省由调用方
        // 决定，此处不特判 0，负零即 0 → 返回 0 由调用方钳制）。
        assert_eq!(lf_height_to_px(0), 0);
    }

    #[test]
    fn logfont_full_request() {
        // 微软雅黑 -16 Bold：px=16，族内有 Bold → 不合成。
        let r = resolve_logfont("微软雅黑", -16, FW_BOLD);
        assert_eq!(r.px, 16);
        assert!(r.bold && !r.bold_synthetic);
        // 宋体 -12 Bold：族内无 Bold → 合成标注。
        let r2 = resolve_logfont("宋体", -12, FW_BOLD);
        assert_eq!(r2.px, 12);
        assert!(r2.bold_synthetic);
        // 字重别名：Light 后缀。
        let r3 = resolve_logfont("微软雅黑 Light", -14, FW_NORMAL);
        assert_eq!(r3.px, 14);
        assert_eq!(r3.weight_note, "light-alias");
        // 不存在字体 → fell-back 标注（回退族 + 日志纪律）。
        let r4 = resolve_logfont("Comic Sans MS", -12, FW_NORMAL);
        assert_eq!(r4.weight_note, "fell-back");
        // 中间字重（<600）→ nearest-bold 标注；600（FW_SEMIBOLD）直接映射
        // 最近 Bold 档（VARIX 无半档字重）。
        let r5 = resolve_logfont("Arial", -12, 500);
        assert_eq!(r5.weight_note, "medium-weight-nearest-bold");
        assert!(!r5.bold, "500 < 600 不足加粗线");
        let r6 = resolve_logfont("Arial", -12, FW_SEMIBOLD);
        assert!(r6.bold, "600 映射最近 Bold 档");
        assert_eq!(r6.weight_note, "");
    }
}

// ---------------------------------------------------------------------------
// 深化批次二：自检聚合（主检 + 深化检并为一行——AI-U2 merge 先例；
// robust.rs / 隔离壳 checkup 接线不变，深化检查项全部经由此行可见）。
// ---------------------------------------------------------------------------

/// 域自检（聚合版）。
pub fn run_fontchain_checks() -> CheckSet {
    CheckSet::merge(run_fontchain_base_checks(), CheckSet::merge(run_fontchain_deep_checks(), run_fontchain_deep2_checks()))
}

// ---------------------------------------------------------------------------
// F016 · 深化批次二：字体角色查询 + 高频别名规范化
//
// 主册依据（G-A-16【功能定义】）：「宋体→衬线族、微软雅黑→无衬线族、
// Consolas/Courier→等宽族、Segoe UI→无衬线族」——角色（role）查询面；
// 【设计细节】映射覆盖 20 个高频名（含 Windows 高频别名）——别名规范化
// （别名 → 规范名再查表，别名不属于表键是 Windows 匹配语义）。
// ---------------------------------------------------------------------------

/// 字体角色（E7 字体设置页/程序请求的分类面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontRole {
    Serif,
    Sans,
    Mono,
    Unknown,
}

/// 高频名 → 角色（主册【功能定义】映射序）。
pub fn role_of(win_name: &str) -> FontRole {
    match win_name {
        "宋体" | "Times New Roman" | "Georgia" => FontRole::Serif,
        "微软雅黑" | "黑体" | "Segoe UI" | "Arial" | "Verdana" | "Tahoma" | "楷体" => FontRole::Sans,
        "Consolas" | "Courier New" | "Courier" => FontRole::Mono,
        _ => FontRole::Unknown,
    }
}

/// 高频别名规范化（别名 → 规范名；非别名原样返回——表内键是规范名，
/// 全名直查必落回退，别名规范化是 Windows 匹配语义的前置步骤）。
pub fn canonical_family(win_name: &str) -> &str {
    match win_name {
        "MS Shell Dlg" => "微软雅黑",
        "MS Sans Serif" => "Segoe UI",
        "System" => "Segoe UI",
        "Fixedsys" => "Consolas",
        _ => win_name,
    }
}

/// 别名感知的角色查询（canonical 先行 → role）。
pub fn role_resolved(win_name: &str) -> FontRole {
    role_of(canonical_family(win_name))
}

/// F016 深化自检。
pub fn run_fontchain_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep");
    // 1) 角色映射对齐主册【功能定义】四锚点。
    cs.add(
        "role_mapping_anchors",
        role_of("宋体") == FontRole::Serif
            && role_of("微软雅黑") == FontRole::Sans
            && role_of("Consolas") == FontRole::Mono
            && role_of("Segoe UI") == FontRole::Sans
            && role_of("不存在的字体") == FontRole::Unknown,
        "",
    );
    // 2) 别名规范化：四个 Windows 高频别名落规范名；非别名原样。
    cs.add(
        "alias_canonicalization",
        canonical_family("MS Shell Dlg") == "微软雅黑"
            && canonical_family("MS Sans Serif") == "Segoe UI"
            && canonical_family("System") == "Segoe UI"
            && canonical_family("Fixedsys") == "Consolas"
            && canonical_family("Arial") == "Arial",
        "",
    );
    // 3) 别名感知角色：别名也拿得到正确角色（直查会 Unknown——规范化必要
    //    性的证明锚）。
    cs.add(
        "role_resolved_via_alias",
        role_of("MS Shell Dlg") == FontRole::Unknown && role_resolved("MS Shell Dlg") == FontRole::Sans,
        "",
    );
    // 4) LOGFONT 语义（深化一批既有面）对账锚：负值直取/正值扣内距/600 加粗线。
    cs.add(
        "logfont_anchored",
        lf_height_to_px(-16) == 16 && lf_height_to_px(20) == 18 && resolve_logfont("Arial", -12, FW_SEMIBOLD).bold,
        "",
    );
    cs
}

// ---------------------------------------------------------------------------
// F016 · 深化批次三：合成字重标注（无 Bold 时合成加粗并标注，斜体同策略）+
// 缺字回退链终点 tofu 显式可见 + E7 整体换族（映射跟随用户选择）
//
// 主册依据（G-A-16【设计细节】）：「加粗映射到族内 Bold 字重（无 Bold 时合成
// 加粗并标注）；斜体同策略」；【状态与异常】「字体缺失字符 → 回退链逐级（最终
// tofu 显式可见不静默空白）」；「E7 页可整体换族（映射跟随用户选择）」。
// ---------------------------------------------------------------------------

/// 字面请求（程序请求的字面组合——LOGFONT 既有语义面之上的组合视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceRequest {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

/// 字面解析结果：原生命中或合成（合成必须带标注——用户与日志都看得见）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FaceResolution {
    Native,
    SyntheticBold,
    SyntheticItalic,
    SyntheticBoldItalic,
}

/// 字面解析：族内有对应字重走原生；缺 → 合成 + 标注短语（三要素之「发生了
/// 什么」——合成是降级不是静默替换）。
pub fn resolve_face(req: FaceRequest, has_bold: bool, has_italic: bool) -> (FaceResolution, &'static str) {
    match req {
        FaceRequest::Regular => (FaceResolution::Native, ""),
        FaceRequest::Bold => {
            if has_bold {
                (FaceResolution::Native, "")
            } else {
                (FaceResolution::SyntheticBold, "族内无 Bold 字重，已合成加粗")
            }
        }
        FaceRequest::Italic => {
            if has_italic {
                (FaceResolution::Native, "")
            } else {
                (FaceResolution::SyntheticItalic, "族内无 Italic 字重，已合成斜体")
            }
        }
        FaceRequest::BoldItalic => match (has_bold, has_italic) {
            (true, true) => (FaceResolution::Native, ""),
            (false, true) => (FaceResolution::SyntheticBold, "族内无 Bold 字重，已合成加粗"),
            (true, false) => (FaceResolution::SyntheticItalic, "族内无 Italic 字重，已合成斜体"),
            (false, false) => (
                FaceResolution::SyntheticBoldItalic,
                "族内无 Bold 与 Italic 字重，已合成加粗斜体",
            ),
        },
    }
}

/// tofu 字形（U+25A1 白方块——回退链终点的显式可见落点，不静默空白）。
pub const TOFU_GLYPH: &str = "\u{25A1}";

/// 缺字回退链逐级查找结果：链上命中族 / 走完全链未命中（→ tofu）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphFallback {
    /// 实际尝试的族数。
    pub chain_tried: u8,
    /// 命中族（None = 全链未命中 → tofu 显式呈现）。
    pub resolved_family: Option<&'static str>,
}

/// 逐级回退：`covers(family, ch)` 为字形覆盖查询（渲染管线接口位）。
pub fn glyph_fallback(
    chain: &[&'static str],
    covers: fn(&str, &str) -> bool,
    ch: &str,
) -> GlyphFallback {
    for (i, fam) in chain.iter().enumerate() {
        if covers(fam, ch) {
            return GlyphFallback { chain_tried: i as u8 + 1, resolved_family: Some(fam) };
        }
    }
    GlyphFallback { chain_tried: chain.len() as u8, resolved_family: None }
}

/// E7 整体换族：用户换族后映射跟随（未设置 → 原映射不动）。
#[derive(Clone, Copy, Debug)]
pub struct FamilyOverride {
    pub user_family: Option<&'static str>,
}

impl FamilyOverride {
    pub const fn unset() -> FamilyOverride {
        FamilyOverride { user_family: None }
    }

    /// 映射消费点统一走此函数（一处一事实：换族逻辑只有这一处）。
    pub fn map(&self, canonical: &'static str) -> &'static str {
        self.user_family.unwrap_or(canonical)
    }
}

/// F016 深化批次三自检。
pub fn run_fontchain_deep2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep2");
    // 1) 合成标注：四组合全走——原生无标注、合成必有非空标注（降级零静默）。
    let (r1, n1) = resolve_face(FaceRequest::Bold, true, true);
    let (r2, n2) = resolve_face(FaceRequest::Bold, false, true);
    let (r3, n3) = resolve_face(FaceRequest::Italic, true, false);
    let (r4, n4) = resolve_face(FaceRequest::BoldItalic, false, false);
    cs.add(
        "synthetic_faces_annotated",
        r1 == FaceResolution::Native
            && n1.is_empty()
            && r2 == FaceResolution::SyntheticBold
            && !n2.is_empty()
            && r3 == FaceResolution::SyntheticItalic
            && !n3.is_empty()
            && r4 == FaceResolution::SyntheticBoldItalic
            && !n4.is_empty(),
        "",
    );
    // 2) 缺字回退：第 2 族命中 → chain_tried=2；全链未命中 → tofu 显式落点
    //    （resolved None + chain_tried = 全链长）。
    fn covers_stub(family: &str, ch: &str) -> bool {
        family == "fallback-mono" && ch == "\u{4E2D}"
    }
    let hit = glyph_fallback(&["sans", "fallback-mono", "serif"], covers_stub, "\u{4E2D}");
    let miss = glyph_fallback(&["sans", "serif"], covers_stub, "\u{4E2D}");
    cs.add(
        "glyph_fallback_chain_and_tofu",
        hit.resolved_family == Some("fallback-mono")
            && hit.chain_tried == 2
            && miss.resolved_family.is_none()
            && miss.chain_tried == 2
            && TOFU_GLYPH == "\u{25A1}",
        "",
    );
    // 3) E7 换族：设置后映射跟随用户选择；未设置保持原映射（不影响默认态）。
    let ov = FamilyOverride { user_family: Some("varix-serif") };
    let off = FamilyOverride::unset();
    cs.add(
        "family_override_e7",
        ov.map("varix-sans") == "varix-serif" && off.map("varix-sans") == "varix-sans",
        "",
    );
    cs
}
