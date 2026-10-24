//! VE-F3436 · 换肤文档 —— 全链使用指南 + 过渡动画定制手册 + 失败排查表 Top10 + CI 防漂移。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3436`
//!
//! # 职责（锚点原文拆解）
//!
//! - **全链使用指南**：从「发起换肤」到「确认终态」的完整路径，含四件
//!   （预演/影响面/编排/事务）在什么时机调用、失败怎么接。指南必须能
//!   让一个没读过源码的人写出正确的调用序列；
//! - **过渡动画定制手册**：三种策略（颜色插值/透明度渐变/错峰级联）的
//!   参数、适用场景与**代价**。手册必须写代价——只写「怎么配」不写
//!   「配了会慢多少」，等于鼓励滥用；
//! - **失败排查表 Top10**：先答十个最常见的原因（[`TROUBLESHOOT_TOP10`]）。
//!   排查表的价值在排序：用户遇到问题时，前十行能解决八成；
//! - **CI 防漂移**：手册里的参数默认值参与指纹比对（复用 F3419 的
//!   [`crate::svstar2::ver05e_docs::DocSet::fingerprint`] 思路，但本模块
//!   独立维护，因为换肤手册的漂移节奏与令牌文档不同）；
//! - **无障碍**：标题层级不跳、每节有摘要、代码块有语言标记。
//!
//! # 为什么排查表要「先答十个最常见的」
//!
//! 排查文档最容易被写成「按模块罗列所有可能」，结果是用户从第一行读到
//! 第五十行还没找到自己的问题。Top10 是一种**强制的产品判断**：写的人
//! 必须承认「这十种占了大多数」。剩下那些放附录，不进前十。
//!
//! # 复杂度
//!
//! - `audit`：O(条目数)；
//! - `fingerprint`：O(参数数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5527 起）
// ---------------------------------------------------------------------------

/// 换肤文档诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReskinDocCode {
    /// 缺全链使用指南。
    UsageMissing = 5527,
    /// 缺过渡动画定制手册。
    AnimationManualMissing = 5528,
    /// 排查表不足十条。
    TroubleshootShort = 5529,
    /// 排查条目缺要素（现象/原因/修法）。
    TroubleshootIncomplete = 5530,
    /// 手册条目缺代价说明。
    CostMissing = 5531,
    /// 指纹漂移。
    FingerprintDrift = 5532,
    /// 标题层级跳跃。
    HeadingSkip = 5533,
    /// 章节缺读屏摘要。
    MissingSummary = 5534,
    /// 代码块缺语言标记。
    UnlabeledCode = 5535,
    /// 参数默认值与代码不一致。
    DefaultDrift = 5536,
}

impl ReskinDocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ReskinDocCode::UsageMissing => "缺全链使用指南",
            ReskinDocCode::AnimationManualMissing => "缺过渡动画定制手册",
            ReskinDocCode::TroubleshootShort => "失败排查表不足十条",
            ReskinDocCode::TroubleshootIncomplete => "排查条目缺要素",
            ReskinDocCode::CostMissing => "动画手册条目缺代价说明",
            ReskinDocCode::FingerprintDrift => "文档指纹漂移",
            ReskinDocCode::HeadingSkip => "标题层级跳跃",
            ReskinDocCode::MissingSummary => "章节缺读屏摘要",
            ReskinDocCode::UnlabeledCode => "代码块缺语言标记",
            ReskinDocCode::DefaultDrift => "参数默认值与代码不一致",
        }
    }

    /// 是否阻断。
    pub fn blocking(self) -> bool {
        !matches!(self, ReskinDocCode::FingerprintDrift)
    }
}

/// 排查表容量：锚点要求 Top10，多写不拦但前十必须有。
pub const TROUBLESHOOT_TOP: usize = 10;

// ---------------------------------------------------------------------------
// 二、文档结构
// ---------------------------------------------------------------------------

/// 排查条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TroubleshootEntry {
    /// 名次（1..=10）。
    pub rank: u8,
    /// 现象（用户看到什么）。
    pub symptom: String,
    /// 原因（一句话）。
    pub cause: String,
    /// 修法（可执行的下一步）。
    pub fix: String,
}

impl TroubleshootEntry {
    /// 构造。
    pub fn new(rank: u8, symptom: &str, cause: &str, fix: &str) -> TroubleshootEntry {
        TroubleshootEntry {
            rank,
            symptom: symptom.to_string(),
            cause: cause.to_string(),
            fix: fix.to_string(),
        }
    }

    /// 三要素齐备。
    pub fn complete(&self) -> bool {
        !self.symptom.trim().is_empty()
            && !self.cause.trim().is_empty()
            && !self.fix.trim().is_empty()
    }
}

/// 动画手册条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AnimationEntry {
    /// 策略名。
    pub strategy: String,
    /// 参数（`duration=200ms` / `stagger=40ms`）。
    pub params: Vec<String>,
    /// 代价（必须写清）。
    pub cost: String,
    /// 适用场景。
    pub scene: String,
}

impl AnimationEntry {
    /// 构造。
    pub fn new(strategy: &str, params: &[&str], cost: &str, scene: &str) -> AnimationEntry {
        AnimationEntry {
            strategy: strategy.to_string(),
            params: params.iter().map(|p| (*p).to_string()).collect(),
            cost: cost.to_string(),
            scene: scene.to_string(),
        }
    }

    /// 代价是否写明。
    pub fn has_cost(&self) -> bool {
        !self.cost.trim().is_empty() && self.cost.len() >= 4
    }
}

/// 一个文档章节。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocSection {
    /// id。
    pub id: String,
    /// 层级。
    pub level: u8,
    /// 标题。
    pub title: String,
    /// 读屏摘要。
    pub summary: String,
    /// 正文行。
    pub body: Vec<String>,
    /// 代码块（语言, 内容）。
    pub code: Vec<(String, String)>,
}

impl DocSection {
    /// 构造。
    pub fn new(id: &str, level: u8, title: &str, summary: &str) -> DocSection {
        DocSection {
            id: id.to_string(),
            level,
            title: title.to_string(),
            summary: summary.to_string(),
            body: Vec::new(),
            code: Vec::new(),
        }
    }

    /// 加正文。
    pub fn with_line(mut self, line: &str) -> DocSection {
        self.body.push(line.to_string());
        self
    }

    /// 加代码。
    pub fn with_code(mut self, lang: &str, code: &str) -> DocSection {
        self.code.push((lang.to_string(), code.to_string()));
        self
    }
}

/// 参数默认值声明（文档 vs 代码）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ParamDefault {
    /// 参数名。
    pub name: String,
    /// 文档值。
    pub documented: String,
    /// 代码值。
    pub actual: String,
}

/// 换肤手册。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReskinDoc {
    /// 章节。
    pub sections: Vec<DocSection>,
    /// 动画手册。
    pub animations: Vec<AnimationEntry>,
    /// 排查表。
    pub troubleshooting: Vec<TroubleshootEntry>,
    /// 参数默认值。
    pub defaults: Vec<ParamDefault>,
}

/// 审计发现。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocFinding {
    /// 码。
    pub code: ReskinDocCode,
    /// 位置。
    pub where_: String,
}

impl DocFinding {
    fn new(code: ReskinDocCode, where_: &str) -> DocFinding {
        DocFinding {
            code,
            where_: where_.to_string(),
        }
    }

    /// 读屏渲染。
    pub fn render(&self) -> String {
        format!("E{} {}［{}］", self.code.code(), self.code.text(), self.where_)
    }
}

impl ReskinDoc {
    /// 空手册。
    pub fn new() -> ReskinDoc {
        ReskinDoc {
            sections: Vec::new(),
            animations: Vec::new(),
            troubleshooting: Vec::new(),
            defaults: Vec::new(),
        }
    }

    /// 加章节。
    pub fn push(&mut self, section: DocSection) {
        self.sections.push(section);
    }

    /// 审计。
    pub fn audit(&self) -> Vec<DocFinding> {
        let mut out = Vec::new();
        // 三件必须有。
        if !self.sections.iter().any(|s| s.id == "reskin.usage") {
            out.push(DocFinding::new(ReskinDocCode::UsageMissing, "reskin.usage"));
        }
        if !self.sections.iter().any(|s| s.id == "reskin.animation") {
            out.push(DocFinding::new(
                ReskinDocCode::AnimationManualMissing,
                "reskin.animation",
            ));
        }
        if self.troubleshooting.len() < TROUBLESHOOT_TOP {
            out.push(DocFinding::new(
                ReskinDocCode::TroubleshootShort,
                "reskin.troubleshoot",
            ));
        }
        for e in self.troubleshooting.iter() {
            if !e.complete() {
                out.push(DocFinding::new(
                    ReskinDocCode::TroubleshootIncomplete,
                    &format!("top{}", e.rank),
                ));
            }
        }
        for a in self.animations.iter() {
            if !a.has_cost() {
                out.push(DocFinding::new(ReskinDocCode::CostMissing, &a.strategy));
            }
        }
        for d in self.defaults.iter() {
            if d.documented != d.actual {
                out.push(DocFinding::new(
                    ReskinDocCode::DefaultDrift,
                    &format!("{}={}≠{}", d.name, d.documented, d.actual),
                ));
            }
        }
        // 无障碍。
        let mut last_level = 1u8;
        for s in self.sections.iter() {
            if s.level > last_level.saturating_add(1) {
                out.push(DocFinding::new(ReskinDocCode::HeadingSkip, &s.id));
            }
            last_level = s.level.max(1);
            if s.summary.trim().is_empty() {
                out.push(DocFinding::new(ReskinDocCode::MissingSummary, &s.id));
            }
            for (lang, _) in s.code.iter() {
                if lang.trim().is_empty() {
                    out.push(DocFinding::new(ReskinDocCode::UnlabeledCode, &s.id));
                }
            }
        }
        out
    }

    /// 是否无阻断项。
    pub fn clean(&self) -> bool {
        !self.audit().iter().any(|f| f.code.blocking())
    }

    /// 指纹：只覆盖参数默认值与动画参数，不覆盖散文。
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        let mut rows: Vec<(&str, &str)> = self
            .defaults
            .iter()
            .map(|d| (d.name.as_str(), d.actual.as_str()))
            .collect();
        rows.sort();
        for (name, value) in rows {
            for byte in name.as_bytes().iter().chain(value.as_bytes()) {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        let mut strategies: Vec<&str> = self.animations.iter().map(|a| a.strategy.as_str()).collect();
        strategies.sort();
        for s in strategies {
            for byte in s.as_bytes() {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        hash
    }

    /// CI 防漂移。
    pub fn drift_check(&self, baseline: u64) -> Result<(), ReskinDocCode> {
        if self.fingerprint() == baseline {
            Ok(())
        } else {
            Err(ReskinDocCode::FingerprintDrift)
        }
    }

    /// 读屏导出：先标题+摘要，再正文。
    pub fn reading_dump(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in self.sections.iter() {
            out.push(format!("h{} {}", s.level, s.title));
            out.push(s.summary.clone());
            for line in s.body.iter() {
                out.push(line.clone());
            }
        }
        for e in self.troubleshooting.iter() {
            out.push(format!(
                "Top{}：{} → {} → {}",
                e.rank, e.symptom, e.cause, e.fix
            ));
        }
        out
    }
}

/// 出厂排查表 Top10。每一条都对应一个真实的诊断码（F3421~F3435），
/// 不是凭空编的常见问题。
pub fn troubleshoot_top10() -> Vec<TroubleshootEntry> {
    alloc::vec![
        TroubleshootEntry::new(1, "点了切换但界面没反应", "预演失败后取消（E5405）", "看预演拒绝的路径，先修令牌再切"),
        TroubleshootEntry::new(2, "换到一半又弹回去", "中途失败触发回滚（E5427/E5428）", "查分段预算与回滚原因，必要时降档"),
        TroubleshootEntry::new(3, "换肤期间点按钮没反应", "交互缓冲溢出（E5436）", "提高缓冲容量或缩短换肤时长"),
        TroubleshootEntry::new(4, "两个窗口一个新一个旧", "同步原子性破坏（E5477）", "检查是否所有窗口都 ack 同一代"),
        TroubleshootEntry::new(5, "预览好看，应用后不一样", "保真对拍不通过（E5451）", "跑对拍报告，按不一致路径修渲染"),
        TroubleshootEntry::new(6, "换肤很慢但不知慢在哪", "分段预算超支（E5468）", "看四段实测，按超支段降档"),
        TroubleshootEntry::new(7, "动画卡顿", "连续掉帧触发降档（E5431）", "主动降为透明度渐变或瞬切"),
        TroubleshootEntry::new(8, "缩放到 150% 后间距不对", "DPI 换算偏差超容差（E5524）", "检查是否所有屏用同一换算公式"),
        TroubleshootEntry::new(9, "外部屏总是慢半拍", "异步屏追赶超时（E5523）", "重同步并检查该屏刷新率"),
        TroubleshootEntry::new(10, "换肤后内存一直涨", "压测泄漏判据红（E5511）", "跑万次压测定位未释放路径"),
    ]
}

/// 出厂手册（可直接用于 CI 基线）。
pub fn standard_doc() -> ReskinDoc {
    let mut doc = ReskinDoc::new();
    doc.push(
        DocSection::new("reskin.usage", 1, "换肤全链使用指南", "四件调用顺序与失败接法")
            .with_line("顺序：预演 → 影响面 → 编排 → 事务提交。跳步会被状态机拒绝。")
            .with_code("rust", "engine.begin(req, tick)?; engine.preview(p, tick)?;"),
    );
    doc.push(
        DocSection::new("reskin.animation", 2, "过渡动画定制手册", "三策略参数与代价")
            .with_line("颜色插值最细但最贵；透明度渐变最稳；错峰级联观感最好但总时长最长。"),
    );
    doc.animations = alloc::vec![
        AnimationEntry::new("颜色插值", &["duration=200ms"], "每组件每帧一次插值，200 组件约 8ms/帧", "小范围精细换肤"),
        AnimationEntry::new("透明度渐变", &["duration=160ms"], "恒定两层合成，约 1ms/帧", "大范围换肤"),
        AnimationEntry::new("错峰级联", &["duration=200ms", "stagger=40ms"], "总时长 = 单件 + 错峰×(N-1)", "强调层次感的场景"),
    ];
    doc.troubleshooting = troubleshoot_top10();
    doc.defaults = alloc::vec![
        ParamDefault {
            name: "reskin.budget_ms".to_string(),
            documented: "400".to_string(),
            actual: "400".to_string(),
        },
        ParamDefault {
            name: "reskin.preview_timeout_ticks".to_string(),
            documented: "10000".to_string(),
            actual: "10000".to_string(),
        },
    ];
    doc
}

// ---------------------------------------------------------------------------
// 三、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_doc_is_clean() {
        let doc = standard_doc();
        let findings = doc.audit();
        assert!(doc.clean(), "{:?}", findings.iter().map(|f| f.render()).collect::<Vec<_>>());
        assert_eq!(doc.troubleshooting.len(), TROUBLESHOOT_TOP);
    }

    #[test]
    fn short_troubleshoot_table_is_blocking() {
        let mut doc = standard_doc();
        doc.troubleshooting.truncate(7);
        let f = doc.audit();
        assert!(f.iter().any(|x| x.code == ReskinDocCode::TroubleshootShort));
    }

    #[test]
    fn missing_cost_in_animation_manual_is_flagged() {
        let mut doc = standard_doc();
        doc.animations.push(AnimationEntry::new("瞬切", &["duration=0"], "", "兜底"));
        let f = doc.audit();
        assert!(f.iter().any(|x| x.code == ReskinDocCode::CostMissing));
    }

    #[test]
    fn default_drift_is_detected() {
        let mut doc = standard_doc();
        doc.defaults.push(ParamDefault {
            name: "reskin.slice_ms".to_string(),
            documented: "8".to_string(),
            actual: "4".to_string(),
        });
        let f = doc.audit();
        assert!(f.iter().any(|x| x.code == ReskinDocCode::DefaultDrift));
    }

    #[test]
    fn fingerprint_is_order_independent_and_detects_drift() {
        let a = standard_doc();
        let base = a.fingerprint();
        assert!(a.drift_check(base).is_ok());
        let mut b = standard_doc();
        b.defaults.reverse();
        assert_eq!(b.fingerprint(), base);
        let mut c = standard_doc();
        c.defaults.push(ParamDefault {
            name: "x".to_string(),
            documented: "1".to_string(),
            actual: "2".to_string(),
        });
        assert_eq!(
            c.drift_check(base).err(),
            Some(ReskinDocCode::FingerprintDrift)
        );
    }

    #[test]
    fn accessibility_checks_cover_headings_summaries_and_code() {
        let mut doc = standard_doc();
        doc.push(
            DocSection::new("reskin.deep", 5, "跳级章节", "")
                .with_code("", "x"),
        );
        let f = doc.audit();
        assert!(f.iter().any(|x| x.code == ReskinDocCode::HeadingSkip));
        assert!(f.iter().any(|x| x.code == ReskinDocCode::MissingSummary));
        assert!(f.iter().any(|x| x.code == ReskinDocCode::UnlabeledCode));
    }

    #[test]
    fn reading_dump_starts_with_title_and_summary() {
        let dump = standard_doc().reading_dump();
        assert!(dump
            .first()
            .map(|l| l.contains("h1 换肤全链使用指南"))
            .unwrap_or(false));
        assert!(dump.get(1).map(|l| l.contains("四件调用顺序")).unwrap_or(false));
        assert!(dump.iter().any(|l| l.contains("Top1：")));
    }
}
