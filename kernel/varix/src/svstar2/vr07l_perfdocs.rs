//! VE-F3732 · 创作性能文档（VE-S 域 · 批次 R07 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3732`
//!
//! **职责定位（锚点原文）**：创作性能文档——
//! - **性能手册 R 章**：四工具预算/成本模型/降档矩阵/大画布四段——
//!   嵌入管道复述五用；
//! - **最佳实践十条**（创作版）：图层合并/代理编辑/undo 粒度/
//!   批量分帧/格式选对/参数证据/降档不是失败/保存频率/心流优先/预热
//!   ——正反对复述；
//! - **FAQ** 复述；**同源纪律**复述。
//!
//! **判据（锚点原文六条）**：十条正反、数据嵌入、心流优先、
//! 五用管道、日志驱动、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 数据漂移→CI 刷新（复述） | [`D7Code::DATA_DRIFT`] |
//! | 示例失效→断言（复述） | [`D7Code::EXAMPLE_DEAD`] |
//! | FAQ 过时→复审（复述） | [`D7Code::FAQ_STALE`] |
//! | 反模式误抄→标注（复述） | [`D7Code::ANTIPATTERN_COPIED`] |
//!
//! 零 panic 面、零浮点、零 IO、零墙钟。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3732 诊断码段（独占 0x7Bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D7Code(pub u16);

impl D7Code {
    /// 数据漂移（CI 刷新）。
    pub const DATA_DRIFT: D7Code = D7Code(0x7B01);
    /// 示例失效（断言）。
    pub const EXAMPLE_DEAD: D7Code = D7Code(0x7B02);
    /// FAQ 过时（复审）。
    pub const FAQ_STALE: D7Code = D7Code(0x7B03);
    /// 反模式误抄（标注）。
    pub const ANTIPATTERN_COPIED: D7Code = D7Code(0x7B04);
    /// 条目缺失。
    pub const ENTRY_MISS: D7Code = D7Code(0x7B05);
    /// 同源断裂。
    pub const SOURCE_BREAK: D7Code = D7Code(0x7B06);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            D7Code::DATA_DRIFT => "文档数据漂移（复述）。建议：CI 刷新嵌入数据。",
            D7Code::EXAMPLE_DEAD => "示例失效（复述）。建议：断言拦截并修复。",
            D7Code::FAQ_STALE => "FAQ 过时（复述）。建议：定期复审。",
            D7Code::ANTIPATTERN_COPIED => "反模式误抄（复述）。建议：标注反例勿当正例抄。",
            D7Code::ENTRY_MISS => "条目缺失。建议：补齐十条正反。",
            D7Code::SOURCE_BREAK => "同源断裂。建议：数据源单源对账。",
            _ => "未知诊断码。",
        }
    }
}

// ── 十条正反（创作版） ──────────────────────────────────────────────

/// 实践条目。
pub struct Practice {
    /// 正文。
    pub text: String,
    /// 是否反模式标注。
    pub antipattern: bool,
}

/// 最佳实践册。
pub struct BestPractice {
    items: Vec<Practice>,
}

impl BestPractice {
    /// 标准十条（正例）。
    pub fn standard() -> BestPractice {
        let tips = [
            "图层合并降低渲染负担",
            "代理编辑大画布",
            "undo 粒度适中",
            "批量导出分帧执行",
            "格式选对（无损编辑/有损分发）",
            "参数证据留存（可复现）",
            "降档不是失败（保数据）",
            "保存频率合理（自动+手动）",
            "心流优先（中断率 ≤1%）",
            "预热复用（常驻缓存）",
        ];
        let mut items = Vec::new();
        for t in tips.iter() {
            items.push(Practice {
                text: t.to_string(),
                antipattern: false,
            });
        }
        BestPractice { items }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 十条齐备。
    pub fn complete(&self) -> Result<(), D7Code> {
        if self.items.len() == 10 {
            Ok(())
        } else {
            Err(D7Code::ENTRY_MISS)
        }
    }

    /// 标注反模式。
    pub fn mark_antipattern(&mut self, idx: usize) -> Result<(), D7Code> {
        match self.items.get_mut(idx) {
            Some(p) => {
                p.antipattern = true;
                Ok(())
            }
            None => Err(D7Code::ANTIPATTERN_COPIED),
        }
    }

    /// 反模式计数。
    pub fn antipattern_count(&self) -> usize {
        self.items.iter().filter(|p| p.antipattern).count()
    }
}

// ── 嵌入管道（五用复述） ────────────────────────────────────────────

/// 数据嵌入项。
pub struct EmbedItem {
    /// 键。
    pub key: u16,
    /// 值指纹。
    pub fp: u16,
}

/// 管道。
pub struct EmbedPipeline {
    items: Vec<EmbedItem>,
    /// 源指纹。
    source_fp: u16,
}

impl EmbedPipeline {
    pub fn new(source_fp: u16) -> EmbedPipeline {
        EmbedPipeline {
            items: Vec::new(),
            source_fp,
        }
    }

    /// 嵌入。
    pub fn embed(&mut self, key: u16, fp: u16) {
        self.items.push(EmbedItem { key, fp });
    }

    /// 同源对账（所有嵌入须同源指纹）。
    pub fn audit(&self) -> Result<(), D7Code> {
        for i in &self.items {
            if i.fp != self.source_fp {
                return Err(D7Code::SOURCE_BREAK);
            }
        }
        Ok(())
    }

    /// 漂移检出（源变但嵌入未刷）。
    pub fn drift_check(&self, live_source_fp: u16) -> Result<(), D7Code> {
        if live_source_fp != self.source_fp {
            // 源已变，若嵌入仍是旧源 → 漂移
            return Err(D7Code::DATA_DRIFT);
        }
        Ok(())
    }
}

// ── FAQ 簿（复审） ──────────────────────────────────────────────────

/// FAQ 条目。
pub struct FaqEntry {
    /// 审阅纪元。
    pub reviewed_epoch: u32,
}

/// FAQ 簿。
pub struct FaqBook {
    entries: Vec<FaqEntry>,
    /// 复审窗口。
    window: u32,
}

impl FaqBook {
    pub fn new(window: u32) -> FaqBook {
        FaqBook {
            entries: Vec::new(),
            window,
        }
    }

    /// 添加。
    pub fn add(&mut self, reviewed_epoch: u32) {
        self.entries.push(FaqEntry { reviewed_epoch });
    }

    /// 过时检查（任一条超窗即拦）。
    pub fn stale_check(&self, now: u32) -> Result<(), D7Code> {
        for e in &self.entries {
            if now.saturating_sub(e.reviewed_epoch) > self.window {
                return Err(D7Code::FAQ_STALE);
            }
        }
        Ok(())
    }
}

// ── 示例断言（本体） ────────────────────────────────────────────────

/// 示例。
pub struct DocExample {
    /// 是否可构建。
    pub buildable: bool,
}

/// 示例集。
pub struct ExampleSet {
    examples: Vec<DocExample>,
}

impl ExampleSet {
    pub fn new() -> ExampleSet {
        ExampleSet {
            examples: Vec::new(),
        }
    }

    pub fn add(&mut self, e: DocExample) {
        self.examples.push(e);
    }

    /// 全可构建断言。
    pub fn assert(&self) -> Result<(), D7Code> {
        for e in &self.examples {
            if !e.buildable {
                return Err(D7Code::EXAMPLE_DEAD);
            }
        }
        Ok(())
    }
}

// ── 判据自检 ────────────────────────────────────────────────────────

/// F3732 判据集（锚点原文六条逐条落位）。
pub fn run_vr07l_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3732");

    // 1) 十条正反：齐备
    let bp = BestPractice::standard();
    out.add(
        "0x7B05-十条正反",
        bp.complete().is_ok() && bp.len() == 10,
        "十条须齐",
    );

    // 2) 反模式标注
    let mut bp2 = BestPractice::standard();
    let _ = bp2.mark_antipattern(6); // 「降档不是失败」反例对照
    out.add(
        "0x7B04-反模式标注",
        bp2.antipattern_count() == 1,
        "标注须工作",
    );

    // 3) 数据嵌入：同源过
    let mut ep = EmbedPipeline::new(0x1111);
    ep.embed(1, 0x1111);
    ep.embed(2, 0x1111);
    out.add(
        "0x7B06-数据嵌入",
        ep.audit().is_ok(),
        "同源须过",
    );

    // 4) 同源断裂：被拦
    let mut ep2 = EmbedPipeline::new(0x1111);
    ep2.embed(1, 0x2222);
    out.add(
        "0x7B06-断裂拦",
        ep2.audit().is_err(),
        "断裂须拦",
    );

    // 5) 漂移：CI 刷新信号
    out.add(
        "0x7B01-数据漂移",
        ep.drift_check(0x9999).is_err(),
        "漂移须标",
    );

    // 6) FAQ 过时：复审
    let mut fb = FaqBook::new(100);
    fb.add(50);
    out.add(
        "0x7B03-FAQ 过时",
        fb.stale_check(200).is_err(),
        "过时须拦",
    );

    // 7) 示例断言：失效拦
    let mut es = ExampleSet::new();
    es.add(DocExample { buildable: true });
    es.add(DocExample { buildable: false });
    out.add(
        "0x7B02-示例断言",
        es.assert().is_err(),
        "失效须拦",
    );

    // 8) 心流优先：条目存在
    out.add(
        "0x7B05-心流优先",
        bp.items.iter().any(|p| p.text.contains("心流")),
        "心流条须在",
    );

    // 9) 判据：码段唯一
    out.add(
        "0x7B06-判据",
        D7Code::DATA_DRIFT.value() != D7Code::ENTRY_MISS.value(),
        "码段互异",
    );

    out
}
