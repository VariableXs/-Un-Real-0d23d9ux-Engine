//! VE-F3539 · Persona 一致性 —— 词典一致 + 同款交互 + 随组验证 + E09 指数联动。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3539`
//!
//! # 职责（锚点原文拆解）
//!
//! - **词典一致**（[`PLexicon`]）：个性化交互文案（设置入口/预览
//!   交互/回退路径）从同一张词典取，引用 id 不引用裸文案。
//! - **同款交互**（[`style_check`]）：预览与回退的句式同款
//!   （「域：状态」两段式）。
//! - **随组验证**（[`PConsisRunner`]）：一致性检查随组收口跑。
//! - **E09 指数联动**（[`consis_index`]）：一致性得分参与 E09
//!   质量指数——违例扣分，全绿满分（联动是单向：本册只产出
//!   指数，不消费）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 违例 | 修正 |
//! | 词典更新 | 复检 |
//! | 漏检 | 兜底扫描 |
//!
//! # 复杂度
//!
//! 全部 O(词条 + 引用)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A80 起）
// ---------------------------------------------------------------------------

/// Persona 一致性域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PConsisCode {
    /// 引用未知词条。
    UnknownLexeme = 0x5A80,
    /// 绕过词典的裸文案。
    BypassLexicon = 0x5A81,
    /// 句式不同款。
    StyleMismatch = 0x5A82,
}

impl PConsisCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PConsisCode::UnknownLexeme => "引用未知词条",
            PConsisCode::BypassLexicon => "绕过词典的裸文案",
            PConsisCode::StyleMismatch => "句式不同款",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、词典
// ---------------------------------------------------------------------------

/// 词条 id。
pub type PLexId = u16;

/// 词条。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PLexeme {
    pub id: PLexId,
    pub text: &'static str,
}

/// Persona 词典。
pub struct PLexicon {
    items: Vec<PLexeme>,
    pub version: u32,
}

impl PLexicon {
    /// 内置词条。
    pub fn builtin() -> Self {
        PLexicon {
            items: vec![
                PLexeme { id: 1, text: "个性化" },
                PLexeme { id: 2, text: "预设" },
                PLexeme { id: 3, text: "已恢复默认" },
                PLexeme { id: 4, text: "预览中" },
                PLexeme { id: 5, text: "已回退" },
                PLexeme { id: 6, text: "与无障碍冲突已恢复" },
            ],
            version: 1,
        }
    }

    /// 查词条。
    pub fn get(&self, id: PLexId) -> Option<&PLexeme> {
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
}

// ---------------------------------------------------------------------------
// 三、引用与句式
// ---------------------------------------------------------------------------

/// 一处引用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PLexRef {
    pub site: &'static str,
    pub id: PLexId,
}

/// 引用检查。
pub fn check_refs(lex: &PLexicon, refs: &[PLexRef]) -> Result<(), PConsisCode> {
    for r in refs {
        if lex.get(r.id).is_none() {
            return Err(PConsisCode::UnknownLexeme);
        }
    }
    Ok(())
}

/// 句式检查：「域：状态」两段式。
pub fn style_check(subject: &str, state: &str) -> Result<(), PConsisCode> {
    if subject.trim().is_empty() || state.trim().is_empty() {
        return Err(PConsisCode::StyleMismatch);
    }
    Ok(())
}

/// 兜底扫描：裸文案命中词典 → 判「应引用未引用」。
pub fn sweep_bypass(lex: &PLexicon, literals: &[&str]) -> Vec<&'static str> {
    let mut hits = Vec::new();
    for lit in literals {
        if let Some(l) = lex.items.iter().find(|l| l.text == *lit) {
            hits.push(l.text);
        }
    }
    hits
}

// ---------------------------------------------------------------------------
// 四、E09 指数联动
// ---------------------------------------------------------------------------

/// 一致性得分（千分比）：违例按比例扣分，全绿满分。
/// `checks` 总检查数、`violations` 违例数。
pub fn consis_index(checks: u32, violations: u32) -> u32 {
    if checks == 0 {
        return 0;
    }
    let v = violations.min(checks) as u64;
    ((checks as u64 - v) * 1000 / checks as u64) as u32
}

// ---------------------------------------------------------------------------
// 五、随组运行器
// ---------------------------------------------------------------------------

/// 一致性运行器。
#[derive(Default)]
pub struct PConsisRunner {
    pub last_version: u32,
    pub runs: u32,
}

impl PConsisRunner {
    /// 空跑器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 全查（词典自检 + 引用 + 兜底扫描），产出指数。
    pub fn run(
        &mut self,
        lex: &PLexicon,
        refs: &[PLexRef],
        literals: &[&str],
    ) -> Result<(usize, u32), PConsisCode> {
        if lex.is_empty() {
            return Err(PConsisCode::UnknownLexeme);
        }
        check_refs(lex, refs)?;
        let bypass = sweep_bypass(lex, literals);
        if !bypass.is_empty() {
            return Err(PConsisCode::BypassLexicon);
        }
        self.last_version = lex.version;
        self.runs += 1;
        let violations = 0u32;
        Ok((lex.len(), consis_index(lex.len() as u32, violations)))
    }
}

// ---------------------------------------------------------------------------
// 六、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_ok_and_unknown_blocked() {
        let lex = PLexicon::builtin();
        let refs = [
            PLexRef { site: "entry", id: 1 },
            PLexRef { site: "preview", id: 4 },
        ];
        assert!(check_refs(&lex, &refs).is_ok());
        let bad = [PLexRef { site: "x", id: 999 }];
        assert_eq!(check_refs(&lex, &bad), Err(PConsisCode::UnknownLexeme));
    }

    #[test]
    fn bypass_sweep() {
        let lex = PLexicon::builtin();
        let hits = sweep_bypass(&lex, &["个性化", "custom"]);
        assert_eq!(hits, vec!["个性化"]);
    }

    #[test]
    fn index_scales() {
        assert_eq!(consis_index(10, 0), 1000);
        assert_eq!(consis_index(10, 5), 500);
        assert_eq!(consis_index(0, 0), 0);
    }

    #[test]
    fn runner_green() {
        let mut r = PConsisRunner::new();
        let lex = PLexicon::builtin();
        let refs = [PLexRef { site: "a", id: 2 }];
        assert!(r.run(&lex, &refs, &[]).is_ok());
        assert_eq!(r.runs, 1);
    }
}
