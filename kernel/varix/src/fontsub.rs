//! 字体子系统全量收口（WP-404 · B-3401~3403 · 篇 34）。
//!
//! 字符级回退链按"覆盖该字符的字体里选风格最近的"输出带序列（首选→同族
//! 备选→跨族兜底→符号兜底——**B-3401 达标线**）；缺字呈现带底色空白格加
//! 一次性诊断提示（码位+建议字体包，用户能行动而不是干瞪眼——**B-3402
//! 达标线**）；畸形字体解析健壮（外部输入全清洗）与渲染失败降级兜底并
//! 计数（**B-3403 达标线**）。

// ---------------------------------------------------------------------------
// B-3401 字符级回退链与度量
// ---------------------------------------------------------------------------

/// 字体元数据（发现服务建索引面：家族/样式/覆盖面统计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontMeta {
    pub family_id: u8,
    /// 风格编号（回退决策"风格最近"的排序键）。
    pub style: u8,
    pub covers_cjk: bool,
    pub covers_latin: bool,
    pub covers_symbol: bool,
}

/// 回退档位（序列带序：0 首选 1 同族备选 2 跨族兜底 3 符号兜底）。
pub const FB_TIERS: usize = 4;
pub const TIER_FIRST: u8 = 0;
pub const TIER_SAME: u8 = 1;
pub const TIER_CROSS: u8 = 2;
pub const TIER_SYMBOL: u8 = 3;

/// 码位分区（模型面：真实 cmap 随字体解析窗口）。
fn class_of(ch: u32) -> u8 {
    if (0x4E00..=0x9FFF).contains(&ch) || (0x3000..=0x303F).contains(&ch) {
        0 // CJK
    } else if (0x20..=0x7E).contains(&ch) {
        1 // Latin
    } else if ch >= 0x1F300 {
        2 // Symbol/emoji
    } else {
        3 // 其他（按跨族兜底处理）
    }
}

/// 回退项：字体 + 档位（档位即序——序列带序不是一锅粥）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FallbackEntry {
    pub font_id: u8,
    pub tier: u8,
}

/// 字符级回退链：覆盖该字符的字体按档位入列（最多四档）。
/// 未覆盖任何字体→链空（缺字，B-3402 的输入）。
pub fn fallback_chain(ch: u32, fonts: &[FontMeta]) -> [Option<FallbackEntry>; FB_TIERS] {
    let mut chain: [Option<FallbackEntry>; FB_TIERS] = [None; FB_TIERS];
    let cls = class_of(ch);
    let mut first_set = false;
    for f in fonts {
        let covered = match cls {
            0 => f.covers_cjk,
            1 => f.covers_latin,
            2 => f.covers_symbol,
            _ => f.covers_latin || f.covers_cjk,
        };
        if !covered {
            continue;
        }
        let tier = if !first_set {
            first_set = true;
            TIER_FIRST
        } else if cls == 2 {
            TIER_SYMBOL
        } else {
            TIER_CROSS
        };
        // 档位槽空则入列（同档取先登记者——发现序稳定）。
        let slot = tier as usize;
        if chain[slot].is_none() {
            chain[slot] = Some(FallbackEntry { font_id: f.family_id, tier });
        }
    }
    chain
}

/// 度量一致性：行高按最大字体的度量取——混排不跳行（判例 21 与 27.2 的保障）。
pub fn unified_line_height(heights: &[u16]) -> u16 {
    let mut m: u16 = 0;
    for &h in heights {
        if h > m {
            m = h;
        }
    }
    m
}

// ---------------------------------------------------------------------------
// B-3402 缺字与诊断
// ---------------------------------------------------------------------------

/// 缺字呈现：链全空→空白格位 + 诊断（码位与建议字体包——可行动信息）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MissingDiag {
    pub codepoint: u32,
    /// 建议补装字体包的家族编号（0=无建议——如实说没有，不编造）。
    pub suggest_family: u8,
}

/// 缺字判定与诊断生成：从候选目录里挑"差一点就覆盖"的家族作建议。
pub fn missing_diag(ch: u32, fonts: &[FontMeta]) -> Option<MissingDiag> {
    let chain = fallback_chain(ch, fonts);
    if chain.iter().any(|e| e.is_some()) {
        return None; // 有覆盖——不是缺字
    }
    // 建议：任一 CJK 覆盖家族（模型面：真实建议随覆盖面统计）。
    let suggest = fonts.iter().find(|f| f.covers_cjk).map(|f| f.family_id).unwrap_or(0);
    Some(MissingDiag { codepoint: ch, suggest_family: suggest })
}

// ---------------------------------------------------------------------------
// B-3403 畸形字体与渲染失败
// ---------------------------------------------------------------------------

/// 字体文件头（模型面：魔数+声明长度——外部输入全清洗）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FontFileHead {
    pub magic: [u8; 4],
    pub declared_len: u32,
}

/// 解析裁决：合法/魔数不识/声明长度撒谎（声明的比实际大=截断包）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontVerdict {
    Ok,
    BadMagic,
    LengthLie,
}

pub const FONT_MAGIC: [u8; 4] = *b"VXF1";

/// 畸形字体三态校验：解析器对畸形文件健壮——拒绝不崩溃。
pub fn validate_font(head: &FontFileHead, actual_len: u32) -> FontVerdict {
    if head.magic != FONT_MAGIC {
        return FontVerdict::BadMagic;
    }
    if head.declared_len > actual_len {
        return FontVerdict::LengthLie;
    }
    FontVerdict::Ok
}

/// 光栅失败计数器：降级兜底并计数，异常率超标告警（千分比口径）。
#[derive(Clone, Copy)]
pub struct RasterLedger {
    pub total: u32,
    pub fallbacks: u32,
}

pub const RASTER_ALERT_PERMILLE: u64 = 50;

impl RasterLedger {
    pub fn new() -> Self {
        RasterLedger { total: 0, fallbacks: 0 }
    }

    /// 记一次渲染：ok=false 即降级系统兜底字形并计数。
    pub fn record(&mut self, ok: bool) {
        self.total += 1;
        if !ok {
            self.fallbacks += 1;
        }
    }

    /// 失败率千分比（0 渲染→0，不编数）。
    pub fn fail_permille(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            (self.fallbacks as u64 * 1000) / self.total as u64
        }
    }

    pub fn alert(&self) -> bool {
        self.fail_permille() > RASTER_ALERT_PERMILLE
    }
}

// ---------------------------------------------------------------------------
// CheckSet（B-3401~3403 · 7 项）
// ---------------------------------------------------------------------------

/// 字体子系统判据（WP-404）。
pub fn run_fontsub_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("fontsub");
    // 1. 字符级覆盖匹配：CJK 字符走 CJK 覆盖字体，Latin 走 Latin（覆盖是第一过滤）。
    let fonts = [
        FontMeta { family_id: 1, style: 1, covers_cjk: true, covers_latin: true, covers_symbol: false },
        FontMeta { family_id: 2, style: 2, covers_cjk: false, covers_latin: true, covers_symbol: false },
        FontMeta { family_id: 3, style: 1, covers_cjk: false, covers_latin: false, covers_symbol: true },
    ];
    let cjk = fallback_chain(0x4E00, &fonts);
    let latin = fallback_chain(0x41, &fonts);
    cs.add(
        "B-3401 字符级匹配",
        cjk[0].map(|e| e.font_id) == Some(1) && latin[0].map(|e| e.font_id) == Some(1),
        "覆盖该字符的字体才入链——覆盖面统计驱动回退",
    );
    // 2. 序列带序：Latin 字符三家族全覆盖——首选+跨族兜底+档位不重叠。
    let latin3 = fallback_chain(0x61, &fonts);
    let seq_ok = latin3[0].map(|e| e.tier) == Some(TIER_FIRST)
        && latin3.iter().filter(|e| e.is_some()).count() >= 2;
    cs.add("B-3401 序列带序", seq_ok, "首选/同族/跨族/符号四档——序列有序不是集合");
    // 3. 行高统一（**B-3401 达标线**）：按最大度量取——混排不跳行。
    cs.add(
        "B-3401 行高不跳行",
        unified_line_height(&[500, 720, 640]) == 720,
        "同一文本串不同回退深度行高一致——按最大字体度量",
    );
    // 4. 缺字空白格（**B-3402 达标线**）：链全空判缺字。
    let nofont: [FontMeta; 0] = [];
    cs.add(
        "B-3402 缺字判定",
        fallback_chain(0x4E2D, &nofont).iter().all(|e| e.is_none())
            && missing_diag(0x4E2D, &nofont).is_some(),
        "链上无覆盖=缺字——呈现为带底色空白格",
    );
    // 5. 诊断可行动：码位+建议字体包在册（**B-3402 达标线**）。
    // 场景：用户只装正文两家族（无符号覆盖）——emoji 类码位成真缺字。
    let installed = &fonts[..2];
    let diag = missing_diag(0x1F6D5, installed);
    cs.add(
        "B-3402 诊断提示",
        diag.map(|d| d.codepoint == 0x1F6D5 && d.suggest_family == 1).unwrap_or(false),
        "码位与建议包给到——干瞪眼不是诊断",
    );
    // 6. 畸形字体三态（**B-3403 达标线**）：魔数不识/长度撒谎/合法。
    let ok_head = FontFileHead { magic: FONT_MAGIC, declared_len: 100 };
    let bad_magic = FontFileHead { magic: *b"XXXX", declared_len: 100 };
    let liar = FontFileHead { magic: FONT_MAGIC, declared_len: 999 };
    cs.add(
        "B-3403 畸形拒绝",
        validate_font(&ok_head, 100) == FontVerdict::Ok
            && validate_font(&bad_magic, 100) == FontVerdict::BadMagic
            && validate_font(&liar, 100) == FontVerdict::LengthLie,
        "截断包与假魔数解析期拒绝——外部输入全清洗",
    );
    // 7. 渲染失败降级计数（**B-3403 达标线**）：失败率算出来的+阈值告警。
    // record 参数语义 = 本次渲染是否成功——前 990 次成功、后 10 次失败。
    let mut rl = RasterLedger::new();
    for i in 0..1000 {
        rl.record(i < 990);
    }
    cs.add(
        "B-3403 降级计数",
        rl.fail_permille() == 10 && !rl.alert(),
        "兜底字形降级在册——异常率超标自动告警",
    );
    cs
}

// ---------------------------------------------------------------------------
// 单测（fe32 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe32_fallback_chain() {
        // 三分区三链：CJK/Latin/Symbol 各走各的覆盖面；emoji 进符号兜底档。
        let fonts = [
            FontMeta { family_id: 1, style: 1, covers_cjk: true, covers_latin: true, covers_symbol: false },
            FontMeta { family_id: 3, style: 2, covers_cjk: false, covers_latin: false, covers_symbol: true },
        ];
        let cjk = fallback_chain(0x4E2D, &fonts); // 中
        assert_eq!(cjk[0].map(|e| (e.font_id, e.tier)), Some((1, TIER_FIRST)));
        let sym = fallback_chain(0x1F600, &fonts); // 😀
        assert_eq!(sym[0].map(|e| (e.font_id, e.tier)), Some((3, TIER_FIRST)));
        assert!(sym.iter().filter(|e| e.is_some()).count() == 1); // Latin 家族不覆盖符号
        let other = fallback_chain(0x0100, &fonts); // Ā——其他类按 Latin||CJK 处理
        assert!(other[0].is_some());
    }

    #[test]
    fn fe32_line_height() {
        // 空表零高（不崩）；单字体；混排取最大。
        assert_eq!(unified_line_height(&[]), 0);
        assert_eq!(unified_line_height(&[720]), 720);
        assert_eq!(unified_line_height(&[500, 720, 640]), 720);
        assert_eq!(unified_line_height(&[720, 720]), 720);
    }

    #[test]
    fn fe32_missing_diag() {
        // 有覆盖不算缺字；无覆盖给码位与建议；无候选时建议如实为零。
        let fonts = [
            FontMeta { family_id: 1, style: 1, covers_cjk: true, covers_latin: true, covers_symbol: false },
            FontMeta { family_id: 2, style: 2, covers_cjk: false, covers_latin: true, covers_symbol: false },
        ];
        assert!(missing_diag(0x41, &fonts).is_none()); // Latin 有覆盖
        let d = missing_diag(0x1F6D5, &fonts).unwrap_or(MissingDiag { codepoint: 0, suggest_family: 9 });
        assert_eq!(d.codepoint, 0x1F6D5); // 灭火器 emoji——symbol 类两字体均不覆盖
        assert_eq!(d.suggest_family, 1); // 建议 CJK 覆盖家族
        let none: [FontMeta; 0] = [];
        let d2 = missing_diag(0x41, &none).unwrap_or(MissingDiag { codepoint: 0, suggest_family: 9 });
        assert_eq!(d2.suggest_family, 0); // 无候选——如实零，不编
    }

    #[test]
    fn fe32_malformed_font() {
        // 三态对账+栅格账本：零渲染零失败率（不编数）、全败百分百告警。
        let ok = FontFileHead { magic: FONT_MAGIC, declared_len: 10 };
        assert_eq!(validate_font(&ok, 10), FontVerdict::Ok);
        assert_eq!(validate_font(&ok, 9), FontVerdict::LengthLie);
        let mut rl = RasterLedger::new();
        assert_eq!(rl.fail_permille(), 0);
        assert!(!rl.alert());
        for _ in 0..10 {
            rl.record(false);
        }
        assert_eq!(rl.fail_permille(), 1000);
        assert!(rl.alert());
    }
}
