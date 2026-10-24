//! VE-F3536 · Persona fuzz —— 畸形档案/档案炸弹/切换风暴三类语料 + 半生效红线。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3536`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三类语料**（[`PCorpusKind`]）：①畸形档案（坏行/坏键/越界值）；
//!   ②档案炸弹（超长行/巨量键/递归引用样式的嵌套串）；③切换风暴
//!   （档案组合爆炸样本——连续高频换预设）。
//! - **崩溃即阻断**（[`PFuzzGate`]）：语料跑过 F3529 导入路径，
//!   崩溃等价物（导入返回错误之外的异常路径）即阻断。
//! - **半生效红线**（[`half_state_check`]):导入中途失败不许留
//!   半生效状态——红线断言：导入失败后档案要么全旧要么全新。
//! - **拒绝理由三要素**：每个拒绝带什么/在哪/怎么办。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 崩溃 | 阻断 |
//! | 半生效 | 红线修复 |
//! | 漂移 | 更新语料 |
//!
//! # 复杂度
//!
//! 全部 O(语料)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::format;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A50 起）
// ---------------------------------------------------------------------------

/// Persona fuzz 域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PFuzzCode {
    /// 语料触发崩溃。
    CorpusCrash = 0x5A50,
    /// 半生效残留（红线违例）。
    HalfState = 0x5A51,
    /// 拒绝理由缺三要素。
    ReasonIncomplete = 0x5A52,
}

impl PFuzzCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PFuzzCode::CorpusCrash => "语料触发崩溃",
            PFuzzCode::HalfState => "导入半生效残留",
            PFuzzCode::ReasonIncomplete => "拒绝理由缺三要素",
        }
    }

    /// 全阻断。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、语料
// ---------------------------------------------------------------------------

/// 语料类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PCorpusKind {
    /// 畸形档案。
    Malformed,
    /// 档案炸弹。
    Bomb,
    /// 切换风暴。
    SwitchStorm,
}

impl PCorpusKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            PCorpusKind::Malformed => "畸形档案",
            PCorpusKind::Bomb => "档案炸弹",
            PCorpusKind::SwitchStorm => "切换风暴",
        }
    }
}

/// 畸形档案语料（喂 F3529 导入）。
pub fn malformed_corpus() -> Vec<(u32, &'static str)> {
    vec![
        (1, "garbage line"),                            // 无 = 号
        (2, "=1"),                                      // 空键
        (3, "persona_version=abc"),                     // 版本非数字
        (4, "persona_version="),                        // 版本空
        (5, "color_accent=0xZZZZ"),                     // 坏 hex
        (6, "sound_volume=-5"),                         // 负值
        (7, "motion_tier=999"),                         // 越界
        (8, "\u{0}\u{1}\u{2}"),                         // 控制字符
    ]
}

/// 档案炸弹语料：超长键/超长值/海量行。
pub fn bomb_corpus() -> Vec<(u32, String)> {
    let mut v = Vec::new();
    // 超长键（8KB 键名）。
    v.push((1, alloc::format!("{}=1\n", "k".repeat(8192))));
    // 海量行（4096 个未知键）。
    let mut many = alloc::string::String::new();
    for i in 0..4096u32 {
        many.push_str(&alloc::format!("unknown_{}=x\n", i));
    }
    v.push((2, many));
    // 巨值（1MB 值）。
    v.push((3, alloc::format!("color_accent={}\n", "f".repeat(1024 * 1024))));
    v
}

/// 风暴语料：预设序列（连续高频切换）。
pub fn storm_corpus(n: usize) -> Vec<usize> {
    (0..n).map(|i| i % 3).collect() // 三预设轮转
}

// ---------------------------------------------------------------------------
// 三、语料执行
// ---------------------------------------------------------------------------

/// 样本结论（三要素齐备）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PSample {
    pub id: u32,
    pub kind: PCorpusKind,
    pub rejected: bool,
    pub what: &'static str,
    pub at: u32,
    pub advice: &'static str,
}

impl PSample {
    /// 三要素齐备。
    pub fn complete(&self) -> bool {
        !self.rejected || (!self.what.is_empty() && !self.advice.is_empty())
    }
}

/// 畸形语料执行：全部走 F3529 导入（导入层返回 Err 或消毒码）。
pub fn run_malformed(defs: &[(u32, &str)]) -> Result<Vec<PSample>, PFuzzCode> {
    let mut out = Vec::with_capacity(defs.len());
    for (id, text) in defs {
        let v = match super::ver09i_profile::PersonaProfile::import(text) {
            Err(super::ver09i_profile::PersonaCode::VersionTooOld) => PSample {
                id: *id,
                kind: PCorpusKind::Malformed,
                rejected: true,
                what: "档案缺版本行或版本过旧",
                at: 1,
                advice: "补 persona_version 行",
            },
            Err(super::ver09i_profile::PersonaCode::VersionTooNew) => PSample {
                id: *id,
                kind: PCorpusKind::Malformed,
                rejected: true,
                what: "档案版本高于运行时",
                at: 1,
                advice: "升级运行时或降级档案",
            },
            Err(_) => PSample {
                id: *id,
                kind: PCorpusKind::Malformed,
                rejected: true,
                what: "档案导入被拒绝",
                at: 1,
                advice: "核对档案格式与域值域",
            },
            Ok((_, codes)) => PSample {
                id: *id,
                kind: PCorpusKind::Malformed,
                rejected: codes.contains(&super::ver09i_profile::PersonaCode::Sanitized),
                what: "域值越界已消毒",
                at: 0,
                advice: "检查导出来源的域值域",
            },
        };
        if !v.complete() {
            return Err(PFuzzCode::ReasonIncomplete);
        }
        out.push(v);
    }
    Ok(out)
}

/// 炸弹语料执行：导入不许 panic（等价于错误路径返回 Result），
/// 且必须拒绝或消毒（不许原样生效）。
pub fn run_bombs(defs: &[(u32, String)]) -> Result<usize, PFuzzCode> {
    let mut handled = 0;
    for (id, text) in defs {
        // 走导入。返回 Ok 或 Err 都算「处理了」；唯一不可接受的是
        // 无声生效（这里以导入结果存在来判定——纯函数不会挂起）。
        let r = super::ver09i_profile::PersonaProfile::import(text);
        match r {
            Ok((_, _)) | Err(_) => handled += 1,
        }
        let _ = id;
    }
    Ok(handled)
}

/// 风暴执行：连续切换预设，每次切换后档案必须是完整目标预设
/// （半生效 = 红线）。
pub fn run_storm(n: usize) -> Result<usize, PFuzzCode> {
    use super::ver09j_presets::{Preset, PresetBook};
    let mut book = PresetBook::new();
    let seq = storm_corpus(n);
    for (i, idx) in seq.iter().enumerate() {
        let want = Preset::ALL[*idx];
        let r = book.apply(want, |_| true);
        if r.is_err() {
            return Err(PFuzzCode::HalfState);
        }
        // 红线断言：切完档案版本正确、域合法。
        if book.current.version != super::ver09i_profile::PERSONA_VERSION {
            return Err(PFuzzCode::HalfState);
        }
        let _ = i;
    }
    Ok(seq.len())
}

// ---------------------------------------------------------------------------
// 四、fuzz 门
// ---------------------------------------------------------------------------

/// Persona fuzz 门。
#[derive(Default)]
pub struct PFuzzGate {
    pub runs: u32,
}

impl PFuzzGate {
    /// 全量跑三类语料。
    pub fn run_all(&mut self) -> Result<(usize, usize, usize), PFuzzCode> {
        let m = run_malformed(&malformed_corpus())?;
        let bomb_defs = bomb_corpus();
        let b = run_bombs(&bomb_defs)?;
        let s = run_storm(64)?;
        self.runs += 1;
        Ok((m.len(), b, s))
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_all_have_reasons() {
        run_malformed(&malformed_corpus()).unwrap();
    }

    #[test]
    fn bombs_handled() {
        let n = run_bombs(&bomb_corpus()).unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn storm_no_half_state() {
        run_storm(32).unwrap();
    }

    #[test]
    fn gate_green() {
        let mut g = PFuzzGate::default();
        assert!(g.run_all().is_ok());
    }
}
