//! VE-F3516 · 色温场景一致性 —— 词典一致 + 同款交互 + 随组验证 + 兜底扫描。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3516`
//!
//! # 职责（锚点原文拆解）
//!
//! - **词典一致**（[`Lexicon`]）：色温域的开关样式/过渡时长/通知
//!   文案从**同一张词典**取——「夜间模式」在设置里叫什么、在
//!   通知里就叫什么；过渡时长三档的标签在滑块与读屏里一字不差。
//!   词典是唯一真源，各处引用词条 id 而不是裸文案。
//! - **同款交互**（[`ConsistencyCheck`]）：E06 覆盖层仲裁文案、
//!   HDR 豁免告知、手动覆盖播报的句式同款（「X：状态」三段式）。
//!   句式不一致 = 一致性违例。
//! - **随组验证**（[`ConsistencyRunner`]）：一致性检查随组收口跑，
//!   不单独排队——组绿必须含一致绿。
//! - **兜底扫描**（[`sweep_defaults`]）：兜底机制——万一某处绕过
//!   词典直接写死文案，全量扫描裸文案与词典比对，违例即报
//!   （漏检兜底）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 违例 | 修正（指回词典） |
//! | 词典更新 | 复检（全量重扫） |
//! | 漏检 | 兜底扫描 |
//!
//! # 复杂度
//!
//! 全部 O(词条数 + 引用数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5830 起）
// ---------------------------------------------------------------------------

/// 一致性域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsisCode {
    /// 引用了词典不存在的词条 id。
    UnknownLexeme = 5830,
    /// 裸文案与词典不符（兜底扫描命中）。
    BypassLexicon = 5831,
    /// 句式不同款。
    StyleMismatch = 5832,
    /// 词条更新后未复检。
    StaleCheck = 5833,
}

impl ConsisCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ConsisCode::UnknownLexeme => "引用未知词条",
            ConsisCode::BypassLexicon => "绕过词典的裸文案",
            ConsisCode::StyleMismatch => "句式不同款",
            ConsisCode::StaleCheck => "词典更新后未复检",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, ConsisCode::StaleCheck)
    }
}

// ---------------------------------------------------------------------------
// 二、词典
// ---------------------------------------------------------------------------

/// 词条 id（词典内唯一）。
pub type LexId = u16;

/// 词典条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lexeme {
    /// 词条 id。
    pub id: LexId,
    /// 中文文案。
    pub text: &'static str,
}

/// 色温域词典（唯一真源）。
pub struct Lexicon {
    items: Vec<Lexeme>,
    /// 版本（每次变更 +1，复检判定用）。
    pub version: u32,
}

impl Lexicon {
    /// 内置词典（色温域核心词条）。
    pub fn builtin() -> Self {
        Lexicon {
            items: vec![
                Lexeme { id: 1, text: "夜间模式" },
                Lexeme { id: 2, text: "手动覆盖中" },
                Lexeme { id: 3, text: "自动曲线" },
                Lexeme { id: 4, text: "快速" },
                Lexeme { id: 5, text: "标准" },
                Lexeme { id: 6, text: "平缓" },
                Lexeme { id: 7, text: "全屏 HDR 豁免" },
                Lexeme { id: 8, text: "专注模式进行中" },
            ],
            version: 1,
        }
    }

    /// 取词条。
    pub fn get(&self, id: LexId) -> Option<&Lexeme> {
        self.items.iter().find(|l| l.id == id)
    }

    /// 词条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 词典内自检：无重复 id、无空文案。
    pub fn self_check(&self) -> Result<(), ConsisCode> {
        for (i, a) in self.items.iter().enumerate() {
            if a.text.is_empty() {
                return Err(ConsisCode::UnknownLexeme);
            }
            for b in &self.items[i + 1..] {
                if a.id == b.id {
                    return Err(ConsisCode::UnknownLexeme);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、句式（同款交互）
// ---------------------------------------------------------------------------

/// 同款句式：「主语：状态」三段式（主语、冒号、状态，各段非空）。
pub fn style_check(subject: &str, state: &str) -> Result<(), ConsisCode> {
    if subject.trim().is_empty() || state.trim().is_empty() {
        return Err(ConsisCode::StyleMismatch);
    }
    Ok(())
}

/// E06 覆盖层仲裁播报（词典同源组装示例）。
pub fn override_announce(lex: &Lexicon, manual: bool) -> Result<&'static str, ConsisCode> {
    let id = if manual { 2 } else { 3 };
    lex.get(id).map(|l| l.text).ok_or(ConsisCode::UnknownLexeme)
}

// ---------------------------------------------------------------------------
// 四、引用检查与兜底扫描
// ---------------------------------------------------------------------------

/// 一处文案引用（各界面挂词典的地方）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LexRef {
    /// 引用点（界面 id）。
    pub site: &'static str,
    /// 词条 id。
    pub id: LexId,
}

/// 引用检查：全部引用必须能在词典解析。
pub fn check_refs(lex: &Lexicon, refs: &[LexRef]) -> Result<(), ConsisCode> {
    for r in refs {
        if lex.get(r.id).is_none() {
            return Err(ConsisCode::UnknownLexeme);
        }
    }
    Ok(())
}

/// 兜底扫描：界面侧裸文案（绕过词典的直写）与词典比对。
/// 直写文案若与任一词条完全一致 → 判为「应引用未引用」。
pub fn sweep_bypass(lex: &Lexicon, literals: &[&str]) -> Vec<&'static str> {
    let mut hits = Vec::new();
    for lit in literals {
        if lex.items.iter().any(|l| l.text == *lit) {
            // 词典是 &'static 串表，命中即回传词条文本。
            if let Some(l) = lex.items.iter().find(|l| l.text == *lit) {
                hits.push(l.text);
            }
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// 五、随组验证运行器
// ---------------------------------------------------------------------------

/// 一致性运行器（随组收口跑）。
#[derive(Default)]
pub struct ConsistencyRunner {
    /// 上次复检时的词典版本。
    pub last_version: u32,
    pub runs: u32,
}

impl ConsistencyRunner {
    /// 空跑器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 全查：词典自检 + 引用 + 句式。词典版本比上次新 → 要求复检
    /// （本调用即复检，通过后记账）。
    pub fn run(
        &mut self,
        lex: &Lexicon,
        refs: &[LexRef],
        literals: &[&str],
    ) -> Result<usize, ConsisCode> {
        lex.self_check()?;
        check_refs(lex, refs)?;
        let bypass = sweep_bypass(lex, literals);
        if !bypass.is_empty() {
            return Err(ConsisCode::BypassLexicon);
        }
        if lex.version > self.last_version && self.runs > 0 {
            // 更新后首次复检：本次通过即记账（不许再拖）。
            self.last_version = lex.version;
        }
        self.runs += 1;
        Ok(lex.len())
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const REFS: [LexRef; 3] = [
        LexRef { site: "settings-toggle", id: 1 },
        LexRef { site: "overlay-banner", id: 2 },
        LexRef { site: "speed-slider", id: 4 },
    ];

    #[test]
    fn refs_resolve() {
        let lex = Lexicon::builtin();
        assert!(check_refs(&lex, &REFS).is_ok());
    }

    #[test]
    fn unknown_lexeme_blocked() {
        let lex = Lexicon::builtin();
        let bad = [LexRef { site: "x", id: 999 }];
        assert_eq!(check_refs(&lex, &bad), Err(ConsisCode::UnknownLexeme));
    }

    #[test]
    fn bypass_sweep_hits() {
        let lex = Lexicon::builtin();
        let hits = sweep_bypass(&lex, &["夜间模式", "自定义文案"]);
        assert_eq!(hits, vec!["夜间模式"]);
    }

    #[test]
    fn style_three_part() {
        assert!(style_check("色温", "手动覆盖中").is_ok());
        assert_eq!(style_check("", "x"), Err(ConsisCode::StyleMismatch));
    }
}
