//! CGPU-F0509 · 抑制文档（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 文档：抖动成因白皮书（五源分类/指纹/抑制原理）；均衡器策略指南
//!   （权衡声明/配置建议）；验证协议说明。判据：三文档、判据。
//!
//! 设计纪律：
//! - 三文档模型：DocBook 登记三册——白皮书（五源各一节：指纹特征 +
//!   抑制原理）、策略指南（三策略权衡声明 + 配置建议树）、验证协议
//!   （注入矩阵 + 判定口径）。
//! - 结构校验：每册必含节集合齐全；锚点引用（F 号）必须可解析；术语
//!   必须在词汇表登记（未登记术语=文档债）。
//! - 版本演进：文档版本号单调；内容哈希入册——漂移可检测。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5240 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x5240，批十独占）。
pub const SEG_BASE: u16 = 0x5240;

pub const E_DC_MISS: u16 = SEG_BASE + 1; // 必含节缺失
pub const E_DC_ANCHOR: u16 = SEG_BASE + 2; // 锚点引用不可解析
pub const E_DC_TERM: u16 = SEG_BASE + 3; // 术语未登记
pub const E_DC_VER: u16 = SEG_BASE + 4; // 版本回退

/// 文档标识（三册）。
pub const DC_WHITEPAPER: u8 = 1; // 抖动成因白皮书
pub const DC_GUIDE: u8 = 2; // 策略指南
pub const DC_PROTO: u8 = 3; // 验证协议
pub const DOC_IDS: [u8; 3] = [DC_WHITEPAPER, DC_GUIDE, DC_PROTO];

/// 五源分类（D02 五源分类学，白皮书五节对应）。
pub const FIVE_SOURCES: [&str; 5] = [
    "sched",   // 调度抖动
    "vblank",  // 垂直同步相位漂移
    "power",   // 功耗态切换
    "thermal", // 热降频
    "driver",  // 驱动批处理
];

/// 单册文档。
#[derive(Clone, Debug)]
pub struct DocUnit {
    /// 文档标识。
    pub id: u8,
    /// 版本（单调递增）。
    pub ver: u32,
    /// 节标题集合。
    pub sections: Vec<&'static str>,
    /// 锚点引用（F 号文本）。
    pub anchors: Vec<&'static str>,
    /// 用到的术语。
    pub terms: Vec<&'static str>,
    /// 内容哈希（FNV-1a）。
    pub content_hash: u64,
}

impl DocUnit {
    fn fnv(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 构造并计算内容哈希。
    pub fn build(
        id: u8,
        ver: u32,
        sections: &[&'static str],
        anchors: &[&'static str],
        terms: &[&'static str],
    ) -> Self {
        let mut bytes = Vec::new();
        bytes.push(id);
        bytes.extend_from_slice(&ver.to_le_bytes());
        for s in sections {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(0x1F);
        }
        for a in anchors {
            bytes.extend_from_slice(a.as_bytes());
            bytes.push(0x1E);
        }
        Self {
            id,
            ver,
            sections: sections.to_vec(),
            anchors: anchors.to_vec(),
            terms: terms.to_vec(),
            content_hash: Self::fnv(&bytes),
        }
    }
}

/// 词汇表 + 锚点登记处 + 三册书架。
#[derive(Clone, Debug)]
pub struct DocBook {
    glossary: Vec<&'static str>,
    anchor_registry: Vec<&'static str>,
    shelf: Vec<DocUnit>,
}

impl DocBook {
    pub fn new(glossary: &[&'static str], anchors: &[&'static str]) -> Self {
        Self {
            glossary: glossary.to_vec(),
            anchor_registry: anchors.to_vec(),
            shelf: Vec::new(),
        }
    }

    /// 收册：版本不得回退（同 id 比较）。
    pub fn shelve(&mut self, d: DocUnit) -> Result<(), u16> {
        if let Some(prev) = self.shelf.iter().find(|x| x.id == d.id) {
            if d.ver <= prev.ver {
                return Err(E_DC_VER);
            }
        }
        self.shelf.push(d);
        Ok(())
    }

    /// 结构审计：必含节/锚点可解析/术语已登记。
    pub fn audit(&self, id: u8, required: &[&str]) -> Result<(), u16> {
        let d = match self.shelf.iter().find(|x| x.id == id) {
            Some(d) => d,
            None => return Err(E_DC_MISS),
        };
        for r in required {
            if !d.sections.iter().any(|s| s == r) {
                return Err(E_DC_MISS);
            }
        }
        for a in &d.anchors {
            if !self.anchor_registry.iter().any(|x| x == a) {
                return Err(E_DC_ANCHOR);
            }
        }
        for t in &d.terms {
            if !self.glossary.iter().any(|x| x == t) {
                return Err(E_DC_TERM);
            }
        }
        Ok(())
    }

    /// 白皮书五源覆盖：五源各须出现于节标题。
    pub fn whitepaper_covers_five(&self) -> bool {
        match self.shelf.iter().find(|x| x.id == DC_WHITEPAPER) {
            Some(d) => FIVE_SOURCES
                .iter()
                .all(|s| d.sections.iter().any(|sec| sec.contains(s))),
            None => false,
        }
    }

    pub fn len(&self) -> usize {
        self.shelf.len()
    }
}

/// 三册标准节骨架（规范源）。
pub fn required_sections(id: u8) -> Option<Vec<&'static str>> {
    match id {
        DC_WHITEPAPER => Some(vec![
            "overview",
            "src:sched",
            "src:vblank",
            "src:power",
            "src:thermal",
            "src:driver",
            "suppress-theory",
        ]),
        DC_GUIDE => Some(vec!["tradeoffs", "config-tree", "pitfalls"]),
        DC_PROTO => Some(vec!["inject-matrix", "verdict-criteria"]),
        _ => None,
    }
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb509_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    let glossary: Vec<&'static str> =
        vec!["jitter", "eq", "suppress", "latency", "vsync", "mailbox"];
    let anchors: Vec<&'static str> =
        vec!["CGPU-F0501", "CGPU-F0502", "CGPU-F0503", "CGPU-F0499", "CGPU-F0484"];

    // 1. 三文档齐备：三册全部收架
    let mut book = DocBook::new(&glossary, &anchors);
    let _ = book.shelve(DocUnit::build(
        DC_WHITEPAPER,
        1,
        &["overview", "src:sched", "src:vblank", "src:power", "src:thermal", "src:driver", "suppress-theory"],
        &["CGPU-F0501"],
        &["jitter", "suppress"],
    ));
    let _ = book.shelve(DocUnit::build(
        DC_GUIDE,
        1,
        &["tradeoffs", "config-tree", "pitfalls"],
        &["CGPU-F0502", "CGPU-F0503"],
        &["eq", "latency"],
    ));
    let _ = book.shelve(DocUnit::build(
        DC_PROTO,
        1,
        &["inject-matrix", "verdict-criteria"],
        &["CGPU-F0499", "CGPU-F0484"],
        &["vsync", "mailbox"],
    ));
    v.push(("三文档-三册齐备", book.len() == 3));

    // 2. 白皮书五源覆盖
    v.push(("白皮书-五源五节覆盖", book.whitepaper_covers_five()));

    // 3. 结构审计：三册必含节齐全
    let ok_req = required_sections(DC_WHITEPAPER).map(|r| book.audit(DC_WHITEPAPER, &r) == Ok(())) == Some(true)
        && required_sections(DC_GUIDE).map(|r| book.audit(DC_GUIDE, &r) == Ok(())) == Some(true)
        && required_sections(DC_PROTO).map(|r| book.audit(DC_PROTO, &r) == Ok(())) == Some(true);
    v.push(("结构-三册必含节齐全", ok_req));

    // 4. 缺节即拒：抽掉一节审计红
    let mut b2 = DocBook::new(&glossary, &anchors);
    let _ = b2.shelve(DocUnit::build(
        DC_GUIDE,
        1,
        &["tradeoffs", "config-tree"],
        &[],
        &["eq"],
    ));
    v.push((
        "结构-缺节即拒",
        b2.audit(DC_GUIDE, &["tradeoffs", "config-tree", "pitfalls"]) == Err(E_DC_MISS),
    ));

    // 5. 锚点不可解析即拒
    let mut b3 = DocBook::new(&glossary, &anchors);
    let _ = b3.shelve(DocUnit::build(
        DC_PROTO,
        1,
        &["inject-matrix", "verdict-criteria"],
        &["CGPU-F9999"],
        &[],
    ));
    v.push(("锚点-死链即拒", b3.audit(DC_PROTO, &[]) == Err(E_DC_ANCHOR)));

    // 6. 术语未登记即拒
    let mut b4 = DocBook::new(&glossary, &anchors);
    let _ = b4.shelve(DocUnit::build(
        DC_GUIDE,
        1,
        &["tradeoffs", "config-tree", "pitfalls"],
        &[],
        &["neologism"],
    ));
    v.push(("术语-未登记即拒", b4.audit(DC_GUIDE, &[]) == Err(E_DC_TERM)));

    // 7. 版本回退即拒
    let mut b5 = DocBook::new(&glossary, &anchors);
    let _ = b5.shelve(DocUnit::build(DC_GUIDE, 3, &[], &[], &[]));
    v.push((
        "版本-回退即拒",
        b5.shelve(DocUnit::build(DC_GUIDE, 2, &[], &[], &[])) == Err(E_DC_VER),
    ));

    v
}

pub fn run_cgb509_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb509");
    for (name, ok) in cgb509_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb509_raw_checks as raw;
    #[test]
    fn t_cgb509() {
        for (n, ok) in raw() {
            assert!(ok, "cgb509 check failed: {}", n);
        }
    }
}
