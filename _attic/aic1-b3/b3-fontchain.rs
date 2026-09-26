
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
